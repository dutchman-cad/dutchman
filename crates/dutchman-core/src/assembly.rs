//! Assemblies: the parts and sub-assemblies an assembly places, read from their own files, and
//! their placements.
//!
//! cadmpeg reads an assembly's occurrences and leaves each external part unresolved, with the
//! path the assembly stored for it. That path is usually absolute on the machine that saved
//! it, so a part is looked for at that path, then by its file name next to the assembly. Each
//! part file is read once, however often it is placed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::locate::Finder;
#[cfg(test)]
use crate::locate::PartSearch;
use crate::placement::{compose, inverse, Placement3x4, IDENTITY};
use crate::{Completeness, Error, Format, LeftOutGroup, Model, Report, Severity, Stage};

/// The most part files one assembly may bring in.
pub const MAX_PARTS: usize = 1_000;

/// The most placements one assembly may hold.
pub const MAX_PLACEMENTS: usize = 100_000;

/// How deep sub-assemblies may nest.
pub const MAX_DEPTH: usize = 32;

/// How far from 1 an occurrence's scale may be and still be rigid.
const RIGID: f64 = 1e-9;

/// An assembly's parts and where they are placed.
#[derive(Clone, Debug)]
pub struct Assembly {
    /// Each part, read from its own file, in the order first placed. A sub-assembly is a part
    /// whose model has an assembly of its own.
    pub parts: Vec<Model>,
    /// Each placed part, in the assembly's order.
    pub placements: Vec<Placement>,
    /// Joints between its placements.
    pub joints: Vec<Joint>,
    /// The placements its file records that are not placed, and why.
    pub left_out: Vec<LeftOut>,
    /// The placements (paths, as [`Joint::a`]) held where they are in what holds them: a
    /// design's own root component, which Fusion holds to its document, or to the occurrence
    /// that inserts the design.
    pub held: Vec<Vec<usize>>,
    /// Why each joint the file records was left out or changed, for the writer to report: a
    /// writer that builds that joint some other way leaves its note out.
    pub joint_notes: Vec<JointNote>,
}

/// Why a joint was left out or changed in [`Assembly::joints`].
#[derive(Clone, Debug, PartialEq)]
pub struct JointNote {
    /// What the report says (`joint 3: cylindrical joints are not converted yet`).
    pub text: String,
    /// The joint's kind in the file (`fixed`, `revolute`, `cylindrical`, `grounded`…), when
    /// the note is about one joint's kind.
    pub family: Option<String>,
}

/// A placement an assembly's file records that is not placed.
#[derive(Clone, Debug, PartialEq)]
pub struct LeftOut {
    /// The occurrence's name, when it has one.
    pub name: Option<String>,
    /// The file it places, by name, when it names one.
    pub file: Option<String>,
    /// Why it is left out.
    pub reason: String,
    /// The stage it was lost at.
    pub stage: Stage,
}

impl Assembly {
    /// How much of the assembly is placed: its own placements and, for each placed
    /// sub-assembly, the sub-assembly's, as often as it is placed.
    pub fn completeness(&self) -> Completeness {
        let mut groups: BTreeMap<(Option<String>, String, Stage), u64> = BTreeMap::new();
        let placed = self.count(1, &mut groups, 0);
        let mut left_out: Vec<LeftOutGroup> = groups
            .into_iter()
            .map(|((file, reason, stage), count)| LeftOutGroup {
                file,
                reason,
                stage,
                count,
            })
            .collect();
        left_out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.file.cmp(&b.file)));
        Completeness { placed, left_out }
    }

    /// The placements placed in this assembly placed `times` times, adding those left out to
    /// `groups`.
    fn count(
        &self,
        times: u64,
        groups: &mut BTreeMap<(Option<String>, String, Stage), u64>,
        depth: usize,
    ) -> u64 {
        for left in &self.left_out {
            let count = groups
                .entry((left.file.clone(), left.reason.clone(), left.stage))
                .or_default();
            *count = count.saturating_add(times);
        }
        let mut placed = times.saturating_mul(self.placements.len() as u64);
        if depth >= MAX_DEPTH {
            return placed;
        }
        let mut uses: BTreeMap<usize, u64> = BTreeMap::new();
        for placement in &self.placements {
            *uses.entry(placement.part).or_default() += 1;
        }
        for (part, used) in uses {
            if let Some(sub) = self.parts.get(part).and_then(|part| part.assembly.as_ref()) {
                placed =
                    placed.saturating_add(sub.count(times.saturating_mul(used), groups, depth + 1));
            }
        }
        placed
    }
}

/// A joint between two placed parts. Its frames meet face to face, as two mated faces do:
/// their origins coincide and their z axes oppose, the second frame half a turn about the
/// first's x axis. Its parts are placed where the joint puts them,
/// `A · frame_a · motion · half turn about x = B · frame_b`, with A and B the sides'
/// placements in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    /// The first side: the indices of the placements leading to it, from the top assembly's
    /// [`Assembly::placements`] down through each sub-assembly's; empty for the top assembly
    /// itself, which stays fixed.
    pub a: Vec<usize>,
    /// The second side, the same way.
    pub b: Vec<usize>,
    /// The joint's frame on the first side, in that side's own part.
    pub frame_a: crate::placement::Placement3x4,
    /// The joint's frame on the second side, in that side's own part.
    pub frame_b: crate::placement::Placement3x4,
    /// Whether the source suppresses it.
    pub suppressed: bool,
    /// How the second side moves against the first, and where it is now.
    pub motion: Motion,
    /// Whether the two frames meet with their z axes the same way (the source's frame sense
    /// `aligned`), rather than face to face as two snapped faces do.
    pub flip: bool,
}

/// How a joint lets its second side move, and where it is now.
#[derive(Clone, Debug, PartialEq)]
pub enum Motion {
    /// Not at all: the frames meet.
    Rigid,
    /// Turning about the first frame's z axis, now `angle` radians from where the frames meet,
    /// within `limits` (radians), when it has them. The source's angular offset is in the
    /// first frame; the angle is where the placements put the joint.
    Revolute {
        angle: f64,
        limits: Option<(f64, f64)>,
    },
    /// Sliding along the first frame's z axis, now `distance` mm from where the frames meet.
    Slider { distance: f64 },
}

impl Motion {
    /// The placement of the second frame in the first, where the joint is now.
    pub fn transform(&self) -> crate::placement::Placement3x4 {
        match *self {
            Motion::Rigid => IDENTITY,
            Motion::Revolute { angle, .. } => turn(angle),
            Motion::Slider { distance } => {
                let mut out = IDENTITY;
                out[2][3] = distance;
                out
            }
        }
    }
}

/// Whether a CADIR joint row's connector frames point the same way (its operands'
/// `frame_sense: "aligned"`); absent or `opposed`, they meet face to face.
pub fn frame_aligned(row: &Value) -> bool {
    row.pointer("/operands/frame_sense").and_then(Value::as_str) == Some("aligned")
}

/// Half a turn about x: how the second frame of a joint meets the first.
const FACING: Placement3x4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, -1.0, 0.0, 0.0],
    [0.0, 0.0, -1.0, 0.0],
];

/// How the second frame of a joint meets the first: face to face, or, flipped, with their z
/// axes the same way.
fn facing(flip: bool) -> Placement3x4 {
    if flip {
        IDENTITY
    } else {
        FACING
    }
}

impl Joint {
    /// The placement of the second frame in the first, where the joint is now.
    pub fn meeting(&self) -> Placement3x4 {
        compose(&self.motion.transform(), &facing(self.flip))
    }
}

/// A turn of `angle` radians about z.
fn turn(angle: f64) -> Placement3x4 {
    let (s, c) = angle.sin_cos();
    [[c, -s, 0.0, 0.0], [s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]
}

/// One part placed in an assembly.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// The occurrence's name in the source, when it has one.
    pub name: Option<String>,
    /// Which of [`Assembly::parts`] it places.
    pub part: usize,
    /// Its placement in the assembly: a rigid transform, row-major 3 × 4, in millimetres.
    pub transform: [[f64; 4]; 3],
}

/// Decodes one file, without reading the parts an assembly places.
pub(crate) type Decode<'a> = dyn Fn(Format, &Path, &mut Report) -> Result<Model, Error> + 'a;

/// Reads `input` with `decode` and, for an assembly, the parts and sub-assemblies it places,
/// found by `finder`, inside the assemblies whose files are in `open` (outermost first), so
/// an assembly that places itself, directly or not, is refused instead of read forever.
pub(crate) fn read(
    format: Format,
    input: &Path,
    report: &mut Report,
    decode: &Decode,
    finder: &Finder,
    open: &mut Vec<PathBuf>,
) -> Result<Model, Error> {
    let mut model = decode(format, input, report)?;
    if format.is_assembly() {
        let key = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
        if open.contains(&key) {
            return Err(Error::Assembly(
                "the assembly places itself through its sub-assemblies".to_owned(),
            ));
        }
        if open.len() >= MAX_DEPTH {
            return Err(Error::Assembly(format!(
                "sub-assemblies nest deeper than Dutchman's limit of {MAX_DEPTH}"
            )));
        }
        open.push(key);
        let resolved = resolve(&model, input, report, decode, finder, open);
        open.pop();
        model.assembly = Some(resolved?);
    }
    Ok(model)
}

/// Reads the parts the assembly `model`, read from `input`, places, found by `finder`. A
/// placement whose part cannot be found or read is left out and reported; an assembly with
/// none left is an error.
pub(crate) fn resolve(
    model: &Model,
    input: &Path,
    report: &mut Report,
    decode: &Decode,
    finder: &Finder,
    open: &mut Vec<PathBuf>,
) -> Result<Assembly, Error> {
    let wire = serde_json::to_value(&model.ir.model)
        .map_err(|err| Error::Decode(format!("cannot read the assembly: {err}")))?;
    let mut occurrences: Vec<&Value> = wire
        .get("occurrences")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().collect())
        .unwrap_or_default();
    if occurrences.len() > MAX_PLACEMENTS {
        return Err(Error::Decode(format!(
            "the assembly places {} parts, over Dutchman's limit of {MAX_PLACEMENTS}",
            occurrences.len()
        )));
    }
    occurrences.sort_by_key(|row| row.get("ordinal").and_then(Value::as_u64));

    let folder = input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut parts: Vec<Model> = Vec::new();
    // Each file read, once: its part, or the stage and reason it could not be read.
    let mut by_file: BTreeMap<PathBuf, Result<usize, (Stage, String)>> = BTreeMap::new();
    let mut placements = Vec::new();
    let mut left_out: Vec<LeftOut> = Vec::new();
    let mut nested = 0usize;
    for occurrence in &occurrences {
        let name = occurrence
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let subject = format!("occurrence \"{}\"", name.as_deref().unwrap_or("unnamed"));
        if occurrence.pointer("/parent/kind").and_then(Value::as_str) != Some("root") {
            nested += 1;
            continue;
        }
        let Some(stored) = occurrence
            .pointer("/prototype/document/path")
            .and_then(Value::as_str)
        else {
            leave_out(
                report,
                &mut left_out,
                (&subject, &name, None),
                Stage::Input,
                "its part is not in a separate file Dutchman can find".to_owned(),
            );
            continue;
        };
        let file = file_name(stored);
        let format = match Format::from_path(Path::new(file)) {
            Some(format @ (Format::InventorPart | Format::InventorAssembly)) => format,
            _ => {
                leave_out(
                    report,
                    &mut left_out,
                    (&subject, &name, Some(file)),
                    Stage::Input,
                    format!("{file}: not an Inventor part or assembly"),
                );
                continue;
            }
        };
        let path = match finder.locate(stored, folder) {
            Ok(found) => {
                crate::names::saw_path(&found.path, report);
                if let Some((count, used)) = found.ambiguous {
                    crate::names::saw_stored(&used, report);
                    report.push_at(
                        Stage::Input,
                        Severity::Info,
                        file,
                        format!("{count} files in the project have this name; used \"{used}\""),
                    );
                }
                found.path
            }
            Err(missing) => {
                leave_out(
                    report,
                    &mut left_out,
                    (&subject, &name, Some(file)),
                    Stage::Input,
                    format!("{file}: {missing}"),
                );
                continue;
            }
        };
        let part = match by_file.get(&path) {
            Some(known) => known.clone(),
            None => {
                let read = if parts.len() >= MAX_PARTS {
                    Err((
                        Stage::Input,
                        format!("over Dutchman's limit of {MAX_PARTS} part files"),
                    ))
                } else {
                    let mut part_report = Report::default();
                    let result = read(format, &path, &mut part_report, decode, finder, open)
                        .map_err(|err| (err.stage(), err.to_string()));
                    report.append_prefixed(part_report, file);
                    result.map(|part| {
                        parts.push(part);
                        parts.len() - 1
                    })
                };
                if let Err((stage, why)) = &read {
                    report.push_at(*stage, Severity::Dropped, file, why.clone());
                }
                by_file.insert(path, read.clone());
                read
            }
        };
        let part = match part {
            Ok(part) => part,
            Err((stage, why)) => {
                // The file's own loss says why; this one which placement it costs.
                report.push_at(
                    stage,
                    Severity::Dropped,
                    subject.clone(),
                    format!("{file} could not be read"),
                );
                left_out.push(LeftOut {
                    name: name.clone(),
                    file: Some(file.to_owned()),
                    reason: format!("{file} could not be read: {why}"),
                    stage,
                });
                continue;
            }
        };
        match transform(occurrence) {
            Ok(transform) => placements.push(Placement {
                name,
                part,
                transform,
            }),
            Err(why) => leave_out(
                report,
                &mut left_out,
                (&subject, &name, Some(file)),
                Stage::Map,
                why,
            ),
        }
    }
    if nested > 0 {
        report.push(
            Severity::Info,
            "assembly",
            format!(
                "{nested} occurrences inside sub-assemblies are read from the sub-assembly files"
            ),
        );
    }
    // An assembly that places nothing, as Inventor's own templates do, is empty: there is
    // nothing to search for, so it converts as an empty design.
    if occurrences.is_empty() {
        report.push(
            Severity::Info,
            "assembly",
            "the assembly places no parts: it is empty, as Inventor's assembly templates are",
        );
    } else if placements.is_empty() {
        return Err(Error::Assembly(
            "none of the assembly's parts could be found or read; if they are elsewhere, give \
             the project folder that holds them"
                .to_owned(),
        ));
    }
    Ok(Assembly {
        parts,
        placements,
        joints: Vec::new(),
        left_out,
        held: Vec::new(),
        joint_notes: Vec::new(),
    })
}

/// Reports the placement `who` (its loss subject, its name and the file it places) left out
/// at `stage` for `reason`, and keeps it for the completeness count.
fn leave_out(
    report: &mut Report,
    left_out: &mut Vec<LeftOut>,
    (subject, name, file): (&str, &Option<String>, Option<&str>),
    stage: Stage,
    reason: String,
) {
    report.push_at(stage, Severity::Dropped, subject, reason.clone());
    left_out.push(LeftOut {
        name: name.clone(),
        file: file.map(str::to_owned),
        reason,
        stage,
    });
}

/// Arenas a component's own geometry never reaches into: other components' bodies are kept
/// out by not following `bodies`, and these hold structure and history, not geometry.
pub(crate) const NOT_GEOMETRY: [&str; 6] = [
    "bodies",
    "product_definitions",
    "occurrences",
    "assembly_joints",
    "features",
    "configurations",
];

/// What became of a design's own components.
pub(crate) enum Components {
    /// The design places no components of its own.
    None,
    /// They can be written as an assembly, which carries why each joint was left out.
    Assembly(Assembly),
    /// They cannot, for this reason; the design converts as stored.
    Unplaceable(String),
}

/// The components a design defines and places itself (as Fusion designs do), as an assembly
/// of part models each holding only its component's bodies, plus the design's loose bodies.
pub(crate) fn local(model: &Model) -> Components {
    match local_components(model) {
        Ok(Some((assembly, _))) => Components::Assembly(assembly),
        Ok(None) => Components::None,
        Err(reason) => Components::Unplaceable(reason),
    }
}

/// The components a design defines and places itself, as an assembly of part models each
/// holding only its component's bodies, plus the design's loose bodies; `None` when it places
/// no components of its own, `Err` with the reason when they cannot be placed.
pub fn local_assembly(model: &Model) -> Result<Option<Assembly>, String> {
    Ok(local_components(model)?.map(|(assembly, _)| assembly))
}

/// [`local_assembly`], with why each joint left out was.
fn local_components(model: &Model) -> Result<Option<(Assembly, Vec<String>)>, String> {
    let document = serde_json::to_value(&model.ir).map_err(|err| err.to_string())?;
    let wire = document.get("model").unwrap_or(&Value::Null);
    let arena = |name: &str| {
        wire.get(name)
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    // A design inserted into this one whose contents the file carries (as an .f3z carries
    // its other designs) is an external occurrence holding local ones: it is placed as a
    // sub-assembly of them.
    let all = arena("occurrences");
    let holds_local = |row: &Value| {
        let id = row.get("id").and_then(Value::as_str);
        id.is_some()
            && all.iter().any(|child| {
                scope(child) == Some("local")
                    && child.pointer("/parent/occurrence").and_then(Value::as_str) == id
            })
    };
    let mut occurrences: Vec<&Value> = all
        .iter()
        .filter(|row| match scope(row) {
            Some("local") => true,
            Some("external") => holds_local(row),
            _ => false,
        })
        .collect();
    if occurrences.is_empty() {
        return Ok(None);
    }
    if occurrences.len() > MAX_PLACEMENTS {
        return Err(format!("it places over {MAX_PLACEMENTS} components"));
    }
    occurrences.sort_by_key(|row| row.get("ordinal").and_then(Value::as_u64));

    let definitions: BTreeMap<&str, &Value> = arena("product_definitions")
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let owned: Vec<&str> = definitions
        .values()
        .filter_map(|row| row.get("bodies").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    // Each occurrence's children, by the id of the occurrence they are placed in.
    let mut children: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    let mut roots = Vec::new();
    for occurrence in &occurrences {
        match occurrence.pointer("/parent/kind").and_then(Value::as_str) {
            Some("root") => roots.push(*occurrence),
            _ => {
                let parent = occurrence
                    .pointer("/parent/occurrence")
                    .and_then(Value::as_str)
                    .ok_or("a component placement names no parent")?;
                children.entry(parent).or_default().push(*occurrence);
            }
        }
    }

    // Bodies no component owns are the design's own, placed where they are.
    let loose: Vec<&str> = arena("bodies")
        .iter()
        .filter_map(|row| row.get("id")?.as_str())
        .filter(|id| !owned.contains(id))
        .collect();
    let mut tree = Tree {
        all_owned: loose.is_empty(),
        model,
        document: &document,
        definitions,
        children,
        built: BTreeMap::new(),
        parts: Vec::new(),
        placed: 0,
        open: Vec::new(),
        path: Vec::new(),
        paths: BTreeMap::new(),
    };
    let mut placements = tree.placements(&roots, None)?;
    if tree.placed != occurrences.len() {
        return Err(
            "some components are placed inside components the design does not place".to_owned(),
        );
    }
    let mut parts = std::mem::take(&mut tree.parts);
    let worlds = world_placements(&occurrences)?;
    let inserted = inserted_designs(&occurrences);
    let (joints, mut left_out) = joints(arena("assembly_joints"), &tree.paths, &worlds, &inserted);

    if !loose.is_empty() {
        parts.push(only(model, &document, &loose, model.name.clone())?);
        placements.push(Placement {
            name: None,
            part: parts.len() - 1,
            transform: IDENTITY,
        });
    }
    // A design's root component is held by what holds the design: the document, or the
    // occurrence that inserts it. cadmpeg names its occurrence `root-…`.
    let mut held: Vec<Vec<usize>> = tree
        .paths
        .iter()
        .filter(|(id, _)| id.contains("occurrence#root-"))
        .map(|(_, located)| located.path.clone())
        .collect();
    held.sort();
    held.dedup();
    let mut assembly = Assembly {
        parts,
        placements,
        joints,
        left_out: Vec::new(),
        held,
        joint_notes: Vec::new(),
    };
    left_out.extend(
        place_by_joints(&mut assembly)
            .into_iter()
            .map(|text| JointNote { text, family: None }),
    );
    let texts = left_out.iter().map(|note| note.text.clone()).collect();
    assembly.joint_notes = left_out;
    Ok(Some((assembly, texts)))
}

/// Places the parts the joints join where the joints put them, as cadmpeg's own placements
/// of joined parts may be where they were before the joints were solved. The design itself
/// stays put; from it, each joint places the side not yet placed. A joint neither of whose
/// sides is placed yet keeps its second side and moves its first to it, as Fusion moves the
/// first part picked for a joint to the second. Of a joint between an occurrence and one
/// inside it, the inner one moves, and an occurrence holding one already placed is never
/// moved, as moving it would move what it holds. A joint whose two sides were both placed
/// already is left for SolidCraft to check, with a note when they do not meet. A revolute or
/// slider joint whose parts already turn or slide on its axis where they are placed holds
/// them there instead, at the value their placements give it.
fn place_by_joints(assembly: &mut Assembly) -> Vec<String> {
    let joints = assembly.joints.clone();
    let mut notes = Vec::new();
    let mut fixed: BTreeSet<Vec<usize>> = BTreeSet::from([Vec::new()]);
    let mut pending: Vec<usize> = (0..joints.len()).collect();
    while !pending.is_empty() {
        let next = pending
            .iter()
            .position(|&i| {
                joints
                    .get(i)
                    .is_some_and(|j| fixed.contains(&j.a) || fixed.contains(&j.b))
            })
            .unwrap_or(0);
        let index = pending.remove(next);
        let Some(joint) = joints.get(index) else {
            continue;
        };
        let (Some(a), Some(b)) = (world_at(assembly, &joint.a), world_at(assembly, &joint.b))
        else {
            continue;
        };
        if let Some((motion, frame_b, note)) = as_built(&a, &b, joint) {
            if let Some(row) = assembly.joints.get_mut(index) {
                row.motion = motion;
                row.frame_b = frame_b;
            }
            if let Some(note) = note {
                notes.push(format!("joint {}: {note}", index + 1));
            }
            fixed.insert(joint.a.clone());
            fixed.insert(joint.b.clone());
            continue;
        }
        let reach = compose(&compose(&a, &joint.frame_a), &joint.meeting());
        // An occurrence holds the ones inside it, so moving it to meet one of them moves that
        // one too: of a joint between an occurrence and one inside it, the inner one moves.
        let inside = |outer: &[usize], inner: &[usize]| {
            outer.len() < inner.len() && inner.starts_with(outer)
        };
        let (a_holds_b, b_holds_a) = (inside(&joint.a, &joint.b), inside(&joint.b, &joint.a));
        if a_holds_b && !fixed.contains(&joint.b) {
            fixed.insert(joint.a.clone());
        }
        let (a_fixed, b_fixed) = (fixed.contains(&joint.a), fixed.contains(&joint.b));
        // Never move an occurrence that holds a placed one: check the joint instead.
        let (a_fixed, b_fixed) =
            if (a_fixed && !b_fixed && b_holds_a) || (b_fixed && !a_fixed && a_holds_b) {
                (true, true)
            } else {
                (a_fixed, b_fixed)
            };
        match (a_fixed, b_fixed) {
            (true, true) => {
                let b_frame = compose(&b, &joint.frame_b);
                if !same(&b_frame, &reach) {
                    notes.push(format!(
                        "joint {}: its parts are placed by other joints where it cannot hold them; \
                         SolidCraft reports the conflict",
                        index + 1
                    ));
                }
            }
            (true, false) => {
                set_world_at(
                    assembly,
                    &joint.b,
                    &compose(&reach, &inverse(&joint.frame_b)),
                );
                fixed.insert(joint.b.clone());
            }
            (false, _) => {
                // A goes where B's frame, taken back through the joint, meets A's frame.
                let at = compose(
                    &compose(&compose(&b, &joint.frame_b), &inverse(&joint.meeting())),
                    &inverse(&joint.frame_a),
                );
                set_world_at(assembly, &joint.a, &at);
                fixed.insert(joint.a.clone());
                fixed.insert(joint.b.clone());
            }
        }
    }
    notes
}

/// A joint as it holds its parts where they are placed (`a` and `b`, in the world), when they
/// already turn or slide on its axis there: its frames' z axes parallel or opposed, and their
/// origins apart only along them (to a micrometre); for a fixed joint, its frames' origins
/// and x axes together. Fusion mates a joint's frames opposed or, flipped, aligned, and its
/// files do not say which, so placements that hold a joint either way keep it. Its value is measured
/// from the placements (a revolute's within a half turn either way), and B's frame
/// is taken from where A's meets it, so what else lies between the frames (a revolute's shift
/// along its axis, a slider's turn about it) is kept as built. A revolute's limits are left
/// out, with the reason, when the measured angle lies outside them.
fn as_built(
    a: &Placement3x4,
    b: &Placement3x4,
    joint: &Joint,
) -> Option<(Motion, Placement3x4, Option<String>)> {
    let wa = compose(a, &joint.frame_a);
    // B's frame in A's, taken back through the half turn: on the axis, a turn about z and a
    // shift along it, with z itself kept or reversed.
    let between = compose(
        &compose(&inverse(&wa), &compose(b, &joint.frame_b)),
        &inverse(&facing(joint.flip)),
    );
    let [[r00, _, _, x], [r10, _, _, y], [_, _, r22, z]] = between;
    if (r22.abs() - 1.0).abs() > 1e-9 || x.hypot(y) > 1e-3 {
        return None;
    }
    let mut note = None;
    let motion = match joint.motion {
        Motion::Rigid => {
            if (r00 - 1.0).abs() > 1e-9 || z.abs() > 1e-3 {
                return None;
            }
            Motion::Rigid
        }
        Motion::Revolute { limits, .. } => {
            let angle = r10.atan2(r00);
            let limits = match limits {
                Some((min, max)) if angle < min - 1e-9 || angle > max + 1e-9 => {
                    note = Some("its limits are left out: its angle lies outside them".to_owned());
                    None
                }
                limits => limits,
            };
            Motion::Revolute { angle, limits }
        }
        Motion::Slider { .. } => Motion::Slider { distance: z },
    };
    let meeting = compose(&motion.transform(), &facing(joint.flip));
    let frame_b = compose(&inverse(b), &compose(&wa, &meeting));
    Some((motion, frame_b, note))
}

/// Whether two placements agree, to a micrometre and a nanoradian.
fn same(a: &Placement3x4, b: &Placement3x4) -> bool {
    a.iter().zip(b).all(|(ra, rb)| {
        ra.iter()
            .zip(rb)
            .enumerate()
            .all(|(c, (x, y))| (x - y).abs() <= if c == 3 { 1e-3 } else { 1e-9 })
    })
}

/// The placement in the world of the occurrence a placement path leads to.
fn world_at(assembly: &Assembly, path: &[usize]) -> Option<Placement3x4> {
    let mut world = IDENTITY;
    let mut level = assembly;
    for (depth, index) in path.iter().enumerate() {
        let placement = level.placements.get(*index)?;
        world = compose(&world, &placement.transform);
        if depth + 1 < path.len() {
            level = level.parts.get(placement.part)?.assembly.as_ref()?;
        }
    }
    Some(world)
}

/// Moves the occurrence a placement path leads to so that it is at `world`.
fn set_world_at(assembly: &mut Assembly, path: &[usize], world: &Placement3x4) {
    let Some((last, outer)) = path.split_last() else {
        return;
    };
    let Some(parent) = world_at(assembly, outer) else {
        return;
    };
    let mut level = assembly;
    for index in outer {
        let Some(part) = level.placements.get(*index).map(|p| p.part) else {
            return;
        };
        let Some(next) = level
            .parts
            .get_mut(part)
            .and_then(|part| part.assembly.as_mut())
        else {
            return;
        };
        level = next;
    }
    if let Some(placement) = level.placements.get_mut(*last) {
        placement.transform = compose(&inverse(&parent), world);
    }
}

/// Each occurrence's placement in the world: its own, after those of the occurrences it is
/// placed in.
fn world_placements<'a>(
    occurrences: &[&'a Value],
) -> Result<BTreeMap<&'a str, Placement3x4>, String> {
    let by_id: BTreeMap<&str, &Value> = occurrences
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, *row)))
        .collect();
    let mut out = BTreeMap::new();
    for (id, row) in &by_id {
        let mut world = transform(row)?;
        let mut parent = row.pointer("/parent/occurrence").and_then(Value::as_str);
        let mut depth = 0;
        while let Some(up) = parent {
            depth += 1;
            let row = by_id.get(up).filter(|_| depth <= MAX_DEPTH);
            let Some(row) = row else {
                break;
            };
            world = compose(&transform(row)?, &world);
            parent = row.pointer("/parent/occurrence").and_then(Value::as_str);
        }
        out.insert(*id, world);
    }
    Ok(out)
}

/// The joints that can be written between placed parts, and a note for each that cannot.
/// `paths` gives where each occurrence is placed; `worlds`, its placement in the world as
/// cadmpeg stores it.
fn joints(
    rows: &[Value],
    paths: &BTreeMap<&str, Located>,
    worlds: &BTreeMap<&str, Placement3x4>,
    inserted: &BTreeMap<&str, &str>,
) -> (Vec<Joint>, Vec<JointNote>) {
    let mut joints: Vec<Joint> = Vec::new();
    // Whether each joint is in a copy of a component placed more than once, other than the
    // first.
    let mut copied: Vec<bool> = Vec::new();
    let mut notes = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let family = family(row);
        let mut note = |text: String| {
            notes.push(JointNote {
                text: format!("joint {}: {text}", index + 1),
                family: family.clone(),
            });
        };
        match joint(row, paths, worlds, inserted) {
            Ok((joint, changed, copy)) => {
                // A component's own joint is stored again in each copy of the component; the
                // copies hold one component, so they are one joint of it.
                let again = joints.iter().zip(&copied).any(|(other, other_copy)| {
                    (copy || *other_copy) && other.a == joint.a && other.b == joint.b
                });
                if again {
                    continue;
                }
                if let Some(changed) = changed {
                    note(changed);
                }
                joints.push(joint);
                copied.push(copy);
            }
            Err(reason) => note(reason),
        }
    }
    (joints, notes)
}

/// A joint row's kind: `grounded` for a joint to the ground, else its pair kind.
fn family(row: &Value) -> Option<String> {
    let operands = row.get("operands")?;
    match operands.get("arity").and_then(Value::as_str) {
        Some("grounded") => Some("grounded".to_owned()),
        _ => operands
            .pointer("/kind/kind")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

/// What a joint holds when it joins a part inside one copy of a component placed more than
/// once to something outside that copy.
const WHOLE_COPY: &str =
    "it holds the whole copy of the component placed more than once that its part is in";

/// One joint row as a joint, with a note when it is changed, and whether it is in a copy of a
/// component placed more than once other than the first.
fn joint(
    row: &Value,
    paths: &BTreeMap<&str, Located>,
    worlds: &BTreeMap<&str, Placement3x4>,
    inserted: &BTreeMap<&str, &str>,
) -> Result<(Joint, Option<String>, bool), String> {
    let operands = row.get("operands").unwrap_or(&Value::Null);
    if operands.get("arity").and_then(Value::as_str) == Some("grounded") {
        let operand = operands
            .pointer("/connector/operand")
            .ok_or("it is malformed")?;
        let (located, _, world) = side_path(operand, paths, worlds, inserted)?;
        if located.path.is_empty() {
            return Err("it joins a component to itself".to_owned());
        }
        // Grounding one copy would ground every copy of the component that holds it.
        if located.shared.is_some() {
            return Err("it grounds a component inside one placed more than once".to_owned());
        }
        // Grounding fixes the occurrence where it is stored. Use the same rigid joint to
        // the design as a paired joint to the root, with frames that preserve its placement.
        return Ok((
            Joint {
                a: Vec::new(),
                b: located.path,
                frame_a: world,
                frame_b: FACING,
                flip: false,
                suppressed: row
                    .get("suppressed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                motion: Motion::Rigid,
            },
            None,
            false,
        ));
    }
    if operands.get("arity").and_then(Value::as_str) != Some("pair") {
        return Err("its operands are not supported yet".to_owned());
    }
    let kind = operands.get("kind").unwrap_or(&Value::Null);
    let side = |index: usize| -> Result<(Located, Placement3x4, Placement3x4), String> {
        let connector = operands
            .pointer(&format!("/connectors/{index}"))
            .ok_or("it is malformed")?;
        let frame = connector
            .get("frame")
            .and_then(matrix)
            .ok_or("its connector frames are malformed")?;
        let (located, container, side) = side_path(
            connector.get("operand").unwrap_or(&Value::Null),
            paths,
            worlds,
            inserted,
        )?;
        // The frame is in its operand's own container; the side is the deepest occurrence
        // the operand names, inside that container. Both are kept in the world.
        Ok((located, compose(&container, &frame), side))
    };
    let ((mut a, a_frame, mut a_at), (mut b, b_frame, mut b_at)) = (side(0)?, side(1)?);
    // A joint inside one copy of a component placed more than once is that component's own,
    // written through its first copy. A joint from inside a copy to something outside it
    // cannot hold that part alone, as every copy holds it: it holds the whole copy, which
    // its part is fixed in, where the part is.
    let mut lifted = false;
    let whole_copy = |side: &Located, other: &Located| {
        let depth = side.shared?;
        if side.own.get(..depth) == other.own.get(..depth) {
            return None;
        }
        let own = side.own.get(..depth)?;
        paths
            .iter()
            .find(|(_, holder)| holder.own == own)
            .map(|(id, holder)| (holder.clone(), worlds.get(id).copied().unwrap_or(IDENTITY)))
    };
    if let Some((holder, at)) = whole_copy(&a, &b) {
        (a, a_at, lifted) = (holder, at, true);
    }
    if let Some((holder, at)) = whole_copy(&b, &a) {
        (b, b_at, lifted) = (holder, at, true);
    }
    for depth in [a.shared, b.shared].into_iter().flatten() {
        if a.own.get(..depth) != b.own.get(..depth) {
            return Err(
                "it joins a component inside one placed more than once to one outside that copy"
                    .to_owned(),
            );
        }
    }
    let (frame_a, frame_b) = (
        compose(&inverse(&a_at), &a_frame),
        compose(&inverse(&b_at), &b_frame),
    );
    let copy = a.own != a.path || b.own != b.path;
    let (a, b) = (a.path, b.path);
    if a == b {
        return Err("it joins a component to itself".to_owned());
    }
    let number = |key: &str| kind.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let mut note = None;
    let (motion, frame_a) = match kind
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
    {
        "fixed" => {
            // The offset carries B's frame from A's: along A's frame, then turned about z.
            let t = kind
                .get("translation_offset")
                .and_then(Value::as_array)
                .map(|t| [0, 1, 2].map(|i| t.get(i).and_then(Value::as_f64).unwrap_or(0.0)))
                .unwrap_or([0.0; 3]);
            let mut offset = turn(number("angle"));
            for (row, value) in offset.iter_mut().zip(t) {
                row[3] = value;
            }
            (Motion::Rigid, compose(&frame_a, &offset))
        }
        "revolute" => {
            let limits = match kind.get("angular_limits") {
                None | Some(Value::Null) => None,
                Some(limits) => match limits.get("bounds").and_then(Value::as_str) {
                    Some("range") => {
                        let bound = |key: &str| limits.get(key).and_then(Value::as_f64);
                        match (bound("minimum"), bound("maximum")) {
                            (Some(min), Some(max)) => Some((min, max)),
                            _ => return Err("its limits are malformed".to_owned()),
                        }
                    }
                    _ => {
                        note = Some("its limit on one side only is left out".to_owned());
                        None
                    }
                },
            };
            // The source's angle aligns the frames, as a fixed joint's does: it turns A's
            // frame about z. Where the joint turns from there is in the placements alone, so
            // it starts at none and is measured from them once the parts are placed.
            (
                Motion::Revolute { angle: 0.0, limits },
                compose(&frame_a, &turn(number("angle"))),
            )
        }
        "slider" => (
            Motion::Slider {
                distance: number("distance"),
            },
            frame_a,
        ),
        other => return Err(format!("{other} joints are not converted yet")),
    };
    Ok((
        Joint {
            a,
            b,
            frame_a,
            frame_b,
            suppressed: row
                .get("suppressed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            motion,
            flip: frame_aligned(row),
        },
        match (lifted, note) {
            (true, Some(note)) => Some(format!("{WHOLE_COPY}; {note}")),
            (true, None) => Some(WHOLE_COPY.to_owned()),
            (false, note) => note,
        },
        copy,
    ))
}

/// A placement stored as rows, `[[r00, r01, r02, tx], …]`.
fn matrix(rows: &Value) -> Option<Placement3x4> {
    let rows = rows.as_array().filter(|rows| rows.len() == 3)?;
    let mut out = [[0.0; 4]; 3];
    for (row, values) in out.iter_mut().zip(rows) {
        let values = values.as_array().filter(|values| values.len() == 4)?;
        for (cell, value) in row.iter_mut().zip(values) {
            *cell = value.as_f64().filter(|v| v.is_finite())?;
        }
    }
    Some(out)
}

/// The values of a list, or none.
fn listed(value: Option<&Value>) -> &[Value] {
    value
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

/// The placement path of one side of a joint (the design itself for its root, else the
/// deepest occurrence the operand names), with the world placements, as stored, of the
/// operand's container and of that occurrence.
fn side_path(
    operand: &Value,
    paths: &BTreeMap<&str, Located>,
    worlds: &BTreeMap<&str, Placement3x4>,
    inserted: &BTreeMap<&str, &str>,
) -> Result<(Located, Placement3x4, Placement3x4), String> {
    let container = operand.get("container").unwrap_or(&Value::Null);
    match container.get("container").and_then(Value::as_str) {
        Some("root") => Ok((Located::default(), IDENTITY, IDENTITY)),
        Some("occurrence") => {
            let id = container
                .get("occurrence")
                .and_then(Value::as_str)
                .ok_or("it is malformed")?;
            // Occurrences nested inside the named one are listed by their keys, outermost
            // first (as `occurrence_path`, or as the object's subelements): the deepest one
            // placed wins. Their ids are built the same way as its, or, inside a design
            // inserted into it, end in their key.
            let object = operand.get("object").and_then(Value::as_str).unwrap_or("");
            let prefix = id.strip_suffix(object).filter(|_| !object.is_empty());
            let below = paths.get(id).map(|located| located.own.as_slice());
            let inside = |key: &str| {
                let below = below?;
                let mut found = paths.iter().filter(|(nested, located)| {
                    nested.rsplit(['#', ':']).next() == Some(key)
                        && located.own.len() > below.len()
                        && located.own.starts_with(below)
                });
                match (found.next(), found.next()) {
                    (Some((nested, _)), None) => Some(*nested),
                    _ => None,
                }
            };
            let nested = listed(container.get("occurrence_path"))
                .iter()
                .chain(listed(operand.get("subelements")))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .filter_map(Value::as_str)
                .find_map(|key| {
                    prefix
                        .and_then(|prefix| {
                            let nested = format!("{prefix}{key}");
                            paths.get_key_value(nested.as_str()).map(|(id, _)| *id)
                        })
                        .or_else(|| inside(key))
                });
            let named = nested.unwrap_or(id);
            let located = paths
                .get(named)
                .ok_or("it joins a component the design does not place")?;
            let world = |id: &str| worlds.get(id).copied().unwrap_or(IDENTITY);
            Ok((located.clone(), world(id), world(named)))
        }
        Some("external") => {
            // A component inside a design inserted into this one, whose contents the file
            // carries: the operand names the occurrences inside it by their keys, outermost
            // first. Its frame is in the deepest of them, and the joint holds the occurrence
            // that inserts the design: the inserted design's own joints are not carried, so
            // its parts move as one.
            let named = listed(operand.get("subelements"))
                .iter()
                .rev()
                .filter_map(Value::as_str)
                .find_map(|key| {
                    let mut found = inserted
                        .keys()
                        .filter(|id| id.rsplit_once('#').map(|(_, own)| own) == Some(key));
                    match (found.next(), found.next()) {
                        (Some(id), None) => Some(*id),
                        _ => None,
                    }
                })
                .ok_or("it joins a component of another design")?;
            let holder = inserted.get(named).copied().unwrap_or(named);
            let located = paths
                .get(holder)
                .ok_or("it joins a component the design does not place")?;
            let world = |id: &str| worlds.get(id).copied().unwrap_or(IDENTITY);
            Ok((located.clone(), world(named), world(holder)))
        }
        _ => Err("it is malformed".to_owned()),
    }
}

/// A definition built into a part: the part, the components it holds (each one's definition
/// and placement), which every other occurrence of it at that level must match, and the
/// placement path of the occurrence it was built for.
type Built = (usize, Vec<(String, [[f64; 4]; 3])>, Vec<usize>);

/// Where an occurrence is placed.
#[derive(Clone, Debug, Default, PartialEq)]
struct Located {
    /// Its placement path, through the first copy of each component placed more than once that
    /// holds it: the copy a joint inside such a component is written through.
    path: Vec<usize>,
    /// Its own placement path.
    own: Vec<usize>,
    /// The length of the path to the outermost component placed more than once that holds it,
    /// when one does.
    shared: Option<usize>,
}

/// A design's occurrence tree, turned into parts and sub-assemblies one definition at a time.
struct Tree<'a> {
    model: &'a Model,
    document: &'a Value,
    definitions: BTreeMap<&'a str, &'a Value>,
    /// Occurrences placed inside each occurrence, by its id.
    children: BTreeMap<&'a str, Vec<&'a Value>>,
    /// The definitions built so far at the level being built.
    built: BTreeMap<&'a str, Built>,
    /// The parts of the level being built.
    parts: Vec<Model>,
    /// How many occurrences have been placed, to find those placed in nothing the design places.
    placed: usize,
    /// The occurrences being built, outermost first, to refuse a placement inside itself.
    open: Vec<&'a str>,
    /// The placement indices leading to the occurrence being built.
    path: Vec<usize>,
    /// Where each occurrence is placed.
    paths: BTreeMap<&'a str, Located>,
    /// Whether every body of the design belongs to a component, so that a component naming
    /// no bodies and holding none is empty rather than one cadmpeg could not attribute.
    all_owned: bool,
}

impl<'a> Tree<'a> {
    /// The placements of `occurrences` among `self.parts`; `shared`, the length of the path to
    /// the outermost component placed more than once that holds them, when one does.
    fn placements(
        &mut self,
        occurrences: &[&'a Value],
        shared: Option<usize>,
    ) -> Result<Vec<Placement>, String> {
        let definition = |row: &Value| definition_key(row).map(str::to_owned);
        let mut placements = Vec::with_capacity(occurrences.len());
        for (index, occurrence) in occurrences.iter().enumerate() {
            let copies = occurrences
                .iter()
                .filter(|other| definition(other) == definition(occurrence))
                .count();
            self.path.push(index);
            if let Some(id) = occurrence.get("id").and_then(Value::as_str) {
                let located = Located {
                    path: self.path.clone(),
                    own: self.path.clone(),
                    shared,
                };
                self.paths.insert(id, located);
            }
            // A placement is its own occurrence however many copies sit beside it; what it
            // holds is shared between those copies.
            let inner = if copies > 1 {
                shared.or(Some(self.path.len()))
            } else {
                shared
            };
            let part = self.part(occurrence, inner);
            self.path.pop();
            self.placed += 1;
            placements.push(Placement {
                name: occurrence
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                part: part?,
                transform: transform(occurrence)?,
            });
        }
        Ok(placements)
    }

    /// Counts and records the occurrences inside `id`, an occurrence of a definition already
    /// built at this level, whose own placements are that definition's: `first` is the path of
    /// the copy it matches, and `shared` as for [`Tree::placements`].
    fn reuse_inside(&mut self, id: &str, first: &[usize], shared: Option<usize>) {
        let inner: Vec<&'a Value> = self.children.get(id).cloned().unwrap_or_default();
        for (index, occurrence) in inner.iter().enumerate() {
            self.placed += 1;
            self.path.push(index);
            let mut path = first.to_vec();
            path.push(index);
            if let Some(id) = occurrence.get("id").and_then(Value::as_str) {
                let located = Located {
                    path: path.clone(),
                    own: self.path.clone(),
                    shared,
                };
                self.paths.insert(id, located);
                if self.path.len() <= MAX_DEPTH {
                    self.reuse_inside(id, &path, shared);
                }
            }
            self.path.pop();
        }
    }

    /// The part an occurrence places: its definition's bodies, and, when components are
    /// placed inside it, those as a sub-assembly beside them. `shared` is for the components
    /// inside it, as for [`Tree::placements`].
    fn part(&mut self, occurrence: &'a Value, shared: Option<usize>) -> Result<usize, String> {
        let definition =
            definition_key(occurrence).ok_or("a component placement names no component")?;
        let id = occurrence.get("id").and_then(Value::as_str).unwrap_or("");
        let external = scope(occurrence) == Some("external");
        let inner: Vec<&'a Value> = self.children.get(id).cloned().unwrap_or_default();
        let signature = inner
            .iter()
            .map(|child| {
                let definition = child
                    .pointer("/prototype/definition")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                transform(child).map(|t| (definition, t))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some((part, built_with, first)) = self.built.get(definition) {
            return if *built_with == signature {
                let (part, first) = (*part, first.clone());
                self.reuse_inside(id, &first, shared);
                Ok(part)
            } else {
                Err("one component holds different components where it is placed".to_owned())
            };
        }
        if self.open.contains(&id) || self.open.len() >= MAX_DEPTH {
            return Err("its components nest too deep or inside themselves".to_owned());
        }
        let empty = Value::Null;
        let row = if external {
            &empty
        } else {
            self.definitions
                .get(definition)
                .ok_or("a component placement names a component the design lacks")?
        };
        let bodies: Vec<&str> = row
            .get("bodies")
            .and_then(Value::as_array)
            .map(|bodies| bodies.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let inserted_name = if external {
            inserted_design_name(self.document, occurrence)
        } else {
            None
        };
        let name = inserted_name
            .or_else(|| name_of(row))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{} component {}", self.model.name, self.parts.len() + 1));
        let model = if inner.is_empty() {
            if bodies.is_empty() && self.all_owned {
                only(self.model, self.document, &[], name)?
            } else if bodies.is_empty() {
                return Err(format!(
                    "cadmpeg does not say which bodies belong to component {}",
                    name_of(row).unwrap_or(definition)
                ));
            } else {
                only(self.model, self.document, &bodies, name)?
            }
        } else {
            // A sub-assembly: the components inside it, and its own bodies at its origin.
            let outer = std::mem::take(&mut self.parts);
            let outer_built = std::mem::take(&mut self.built);
            self.open.push(id);
            let placed = self.placements(&inner, shared);
            self.open.pop();
            let mut placements = placed?;
            if !bodies.is_empty() {
                let own = only(self.model, self.document, &bodies, format!("{name} bodies"))?;
                self.parts.push(own);
                placements.push(Placement {
                    name: None,
                    part: self.parts.len() - 1,
                    transform: IDENTITY,
                });
            }
            let parts = std::mem::replace(&mut self.parts, outer);
            self.built = outer_built;
            let mut holder = only(self.model, self.document, &[], name)?;
            holder.assembly = Some(Assembly {
                parts,
                placements,
                joints: Vec::new(),
                left_out: Vec::new(),
                held: Vec::new(),
                joint_notes: Vec::new(),
            });
            holder
        };
        self.parts.push(model);
        let index = self.parts.len() - 1;
        self.built
            .insert(definition, (index, signature, self.path.clone()));
        Ok(index)
    }
}

/// Whether an occurrence places a component of this design (`local`) or another design
/// (`external`).
fn scope(occurrence: &Value) -> Option<&str> {
    occurrence
        .pointer("/prototype/scope")
        .and_then(Value::as_str)
}

/// What makes two placements copies of each other: the component they place, or for a
/// design inserted into this one, the occurrence itself.
fn definition_key(occurrence: &Value) -> Option<&str> {
    match scope(occurrence) {
        Some("external") => occurrence.get("id").and_then(Value::as_str),
        _ => occurrence
            .pointer("/prototype/definition")
            .and_then(Value::as_str),
    }
}

/// Each occurrence inside a design inserted into this one, with the occurrence that inserts
/// that design (the nearest external occurrence above it).
fn inserted_designs<'a>(occurrences: &[&'a Value]) -> BTreeMap<&'a str, &'a str> {
    let by_id: BTreeMap<&str, &Value> = occurrences
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, *row)))
        .collect();
    let mut out = BTreeMap::new();
    for (id, row) in &by_id {
        let mut parent = row.pointer("/parent/occurrence").and_then(Value::as_str);
        let mut depth = 0;
        while let Some(up) = parent {
            depth += 1;
            let Some(row) = by_id.get(up).filter(|_| depth <= MAX_DEPTH) else {
                break;
            };
            if scope(row) == Some("external") {
                out.insert(*id, up);
                break;
            }
            parent = row.pointer("/parent/occurrence").and_then(Value::as_str);
        }
    }
    out
}

/// The name of the design an external occurrence inserts, as the file stores it with the
/// inserted design's file name.
fn inserted_design_name<'v>(document: &'v Value, occurrence: &Value) -> Option<&'v str> {
    let path = occurrence
        .pointer("/prototype/document/path")
        .and_then(Value::as_str)?;
    let file = file_name(path);
    document
        .pointer("/native/f3d/xref_designs")
        .and_then(Value::as_array)?
        .iter()
        .find(|design| design.get("target_file_name").and_then(Value::as_str) == Some(file))
        .and_then(|design| design.get("display_name").and_then(Value::as_str))
        .filter(|name| !name.trim().is_empty())
}

fn name_of(definition: &Value) -> Option<&str> {
    ["label", "source_name"]
        .into_iter()
        .find_map(|key| definition.get(key).and_then(Value::as_str))
        .filter(|name| !name.trim().is_empty())
}

/// `model` reduced to `bodies` and the geometry they reach, named `name`.
pub(crate) fn only(
    model: &Model,
    document: &Value,
    bodies: &[&str],
    name: String,
) -> Result<Model, String> {
    let wire = document
        .get("model")
        .and_then(Value::as_object)
        .ok_or("the model is malformed")?;
    // Every row by id, with its arena.
    let mut rows: BTreeMap<&str, (&str, &Value)> = BTreeMap::new();
    for (arena, values) in wire {
        for row in values.as_array().map_or(&[][..], Vec::as_slice) {
            if let Some(id) = row.get("id").and_then(Value::as_str) {
                rows.insert(id, (arena.as_str(), row));
            }
        }
    }
    let mut keep: BTreeMap<&str, ()> = BTreeMap::new();
    let mut queue: Vec<&str> = Vec::new();
    for body in bodies {
        if rows.contains_key(body) && keep.insert(body, ()).is_none() {
            queue.push(body);
        }
    }
    while let Some(id) = queue.pop() {
        let Some((_, row)) = rows.get(id) else {
            continue;
        };
        let mut refs = Vec::new();
        strings(row, &mut refs);
        for reference in refs {
            if let Some((arena, _)) = rows.get(reference) {
                if !NOT_GEOMETRY.contains(arena) && keep.insert(reference, ()).is_none() {
                    queue.push(reference);
                }
            }
        }
    }
    let mut reduced = serde_json::Map::new();
    for (arena, values) in wire {
        let kept: Vec<Value> = values
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter(|row| {
                row.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| keep.contains_key(id))
            })
            .cloned()
            .collect();
        reduced.insert(arena.clone(), Value::Array(kept));
    }
    let mut part = document.clone();
    if let Some(fields) = part.as_object_mut() {
        fields.insert("model".to_owned(), Value::Object(reduced));
        fields.remove("native");
    }
    // cadmpeg is third-party code; a panic in it is a reason, not a crash.
    let text = part.to_string();
    let ir = std::panic::catch_unwind(|| cadmpeg_ir::CadIr::from_json(&text))
        .map_err(|_| "cadmpeg failed unexpectedly on a component's geometry".to_owned())?
        .map_err(|err| format!("a component's geometry does not stand alone: {err}"))?;
    Ok(Model {
        ir,
        fidelity: None,
        name,
        source_file: model.source_file.clone(),
        assembly: None,
        source_step: None,
    })
}

/// Every string inside `value`.
pub(crate) fn strings<'v>(value: &'v Value, out: &mut Vec<&'v str>) {
    match value {
        Value::String(s) => out.push(s),
        Value::Array(values) => values.iter().for_each(|v| strings(v, out)),
        Value::Object(fields) => fields.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// The file name in a stored path, whichever separators the saving machine used.
fn file_name(stored: &str) -> &str {
    stored.rsplit(['/', '\\']).next().unwrap_or(stored)
}

/// An occurrence's rigid placement, or why it is not one.
fn transform(occurrence: &Value) -> Result<[[f64; 4]; 3], String> {
    let malformed = || "its placement is malformed".to_owned();
    let rows = occurrence
        .get("transform")
        .and_then(Value::as_array)
        .ok_or_else(malformed)?;
    let mut out = [[0.0; 4]; 3];
    for (row, values) in out.iter_mut().zip(rows) {
        let values = values.as_array().ok_or_else(malformed)?;
        for (cell, value) in row.iter_mut().zip(values) {
            *cell = value
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(malformed)?;
        }
    }
    if rows.len() != 3
        || rows
            .iter()
            .any(|row| row.as_array().map(Vec::len) != Some(4))
    {
        return Err(malformed());
    }
    let scaled = occurrence
        .get("scale")
        .and_then(Value::as_array)
        .is_some_and(|scale| {
            scale
                .iter()
                .any(|s| s.as_f64().is_none_or(|s| (s - 1.0).abs() > RIGID))
        });
    if scaled {
        return Err("scaled placements are not carried".to_owned());
    }
    Ok(out)
}

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use serde_json::json;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn occurrence(key: usize, path: &str, parent: Value, x: f64) -> Value {
        json!({
            "id": format!("inventor:assembly:occurrence#{key}"),
            "prototype": {"scope": "external", "document": {"resolution": "path", "path": path}},
            "parent": parent,
            "ordinal": key,
            "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
            "scale": [1.0, 1.0, 1.0],
            "name": format!("Occurrence:{key}"),
        })
    }

    fn root() -> Value {
        json!({"kind": "root"})
    }

    /// A folder of placeholder files and a decoder that gives each its model: the synthetic
    /// Inventor cube for `cube.ipt`, and for an assembly the occurrences `assemblies` lists
    /// for its file name (as `(stored path, x)`).
    struct Project {
        dir: tempfile::TempDir,
        cube: Model,
        assemblies: BTreeMap<String, Vec<Value>>,
    }

    impl Project {
        fn new(assemblies: &[(&str, Vec<Value>)]) -> Project {
            let dir = tempfile::tempdir().unwrap();
            let cube = crate::read(
                Format::InventorPart,
                &fixture("synthetic_cube.ipt"),
                &mut Report::default(),
            )
            .unwrap();
            std::fs::write(dir.path().join("Cube.IPT"), b"").unwrap();
            for (name, _) in assemblies {
                std::fs::write(dir.path().join(name), b"").unwrap();
            }
            let assemblies = assemblies
                .iter()
                .map(|(name, rows)| ((*name).to_owned(), rows.clone()))
                .collect();
            Project {
                dir,
                cube,
                assemblies,
            }
        }

        fn decode(&self, _format: Format, path: &Path, _: &mut Report) -> Result<Model, Error> {
            let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
            if name == "cube.ipt" {
                return Ok(self.cube.clone());
            }
            let rows = self.assemblies.get(&name).cloned().unwrap_or_default();
            let mut wire = serde_json::to_value(&self.cube.ir).unwrap();
            wire["model"]["occurrences"] = Value::Array(rows);
            Ok(Model {
                ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
                fidelity: None,
                name: name.trim_end_matches(".iam").to_owned(),
                source_file: Some(name),
                source_step: None,
                assembly: None,
            })
        }

        fn read(&self, name: &str, report: &mut Report) -> Result<Model, Error> {
            let decode = |format: Format, path: &Path, report: &mut Report| {
                self.decode(format, path, report)
            };
            read(
                Format::InventorAssembly,
                &self.dir.path().join(name),
                report,
                &decode,
                &Finder::new(&PartSearch::project(self.dir.path())),
                &mut Vec::new(),
            )
        }
    }

    fn dropped(report: &Report) -> Vec<String> {
        report
            .losses()
            .iter()
            .filter(|loss| loss.severity == Severity::Dropped)
            .map(|loss| format!("{}: {}", loss.subject, loss.detail))
            .collect()
    }

    #[test]
    fn parts_and_sub_assemblies_are_read_from_their_files() {
        let nested = json!({"kind": "occurrence", "occurrence": "inventor:assembly:occurrence#2"});
        let project = Project::new(&[
            (
                "top.iam",
                vec![
                    occurrence(0, r"C:\Work\Parts\cube.ipt", root(), 0.0),
                    occurrence(1, r"C:\Work\Parts\cube.ipt", root(), 30.0),
                    occurrence(2, r"C:\Work\sub.iam", root(), 100.0),
                    occurrence(3, r"C:\Work\Parts\gone.ipt", root(), 0.0),
                    occurrence(4, r"C:\Work\Parts\cube.ipt", nested, 0.0),
                ],
            ),
            (
                "sub.iam",
                vec![
                    occurrence(0, "cube.ipt", root(), 0.0),
                    occurrence(1, "cube.ipt", root(), 15.0),
                ],
            ),
        ]);
        let mut report = Report::default();
        let top = project.read("top.iam", &mut report).unwrap();
        let assembly = top.assembly.unwrap();
        assert_eq!(
            assembly.parts.len(),
            2,
            "the cube once, and the sub-assembly"
        );
        assert_eq!(assembly.placements.len(), 3);
        assert_eq!(assembly.placements[2].transform[0][3], 100.0);
        let sub = assembly.parts[1].assembly.as_ref().unwrap();
        assert_eq!(sub.parts.len(), 1);
        assert_eq!(sub.placements.len(), 2);
        let completeness = Assembly::completeness(&Assembly {
            parts: assembly.parts.clone(),
            placements: assembly.placements.clone(),
            joints: Vec::new(),
            left_out: assembly.left_out.clone(),
            held: Vec::new(),
            joint_notes: Vec::new(),
        });
        assert_eq!((completeness.placed, completeness.placements()), (5, 6));
        assert_eq!(
            dropped(&report),
            [format!(
                "occurrence \"Occurrence:3\": gone.ipt: not found at its stored path or in the \
                 project folder {}",
                crate::locate::tests::absolute_for_tests(project.dir.path()).display()
            )]
        );
    }

    #[test]
    fn completeness_counts_through_each_placed_sub_assembly() {
        let project = Project::new(&[
            (
                "top.iam",
                vec![
                    occurrence(0, "cube.ipt", root(), 0.0),
                    occurrence(1, "sub.iam", root(), 30.0),
                    occurrence(2, "sub.iam", root(), 60.0),
                    occurrence(3, "gone.ipt", root(), 0.0),
                    occurrence(4, "notes.txt", root(), 0.0),
                ],
            ),
            (
                "sub.iam",
                vec![
                    occurrence(0, "cube.ipt", root(), 0.0),
                    occurrence(1, "cube.ipt", root(), 15.0),
                    occurrence(2, "lost.ipt", root(), 0.0),
                ],
            ),
        ]);
        let top = project.read("top.iam", &mut Report::default()).unwrap();
        let completeness = top.assembly.unwrap().completeness();
        // The cube and the sub-assembly twice at the top; two cubes in each sub-assembly.
        assert_eq!(completeness.placed, 7);
        assert_eq!(completeness.placements(), 11);
        assert_eq!(completeness.left_out_count(), 4);
        let groups: Vec<(Option<&str>, u64, Stage)> = completeness
            .left_out
            .iter()
            .map(|g| (g.file.as_deref(), g.count, g.stage))
            .collect();
        assert_eq!(
            groups,
            [
                (Some("lost.ipt"), 2, Stage::Input),
                (Some("gone.ipt"), 1, Stage::Input),
                (Some("notes.txt"), 1, Stage::Input),
            ]
        );
        assert!(completeness.left_out[0]
            .reason
            .starts_with("lost.ipt: not found"));
    }

    #[test]
    fn an_assembly_that_places_itself_is_refused() {
        let project = Project::new(&[
            (
                "a.iam",
                vec![
                    occurrence(0, "b.iam", root(), 0.0),
                    occurrence(1, "cube.ipt", root(), 0.0),
                ],
            ),
            ("b.iam", vec![occurrence(0, "a.iam", root(), 0.0)]),
        ]);
        let mut report = Report::default();
        let a = project.read("a.iam", &mut report).unwrap();
        assert_eq!(
            a.assembly.unwrap().placements.len(),
            1,
            "the cube is still placed"
        );
        assert!(
            dropped(&report)
                .iter()
                .any(|loss| loss.contains("places itself through its sub-assemblies")),
            "{:?}",
            dropped(&report)
        );
    }

    #[test]
    fn sub_assemblies_nest_only_so_deep() {
        let chain: Vec<(String, Vec<Value>)> = (0..=MAX_DEPTH + 2)
            .map(|n| {
                (
                    format!("d{n}.iam"),
                    vec![
                        occurrence(0, &format!("d{}.iam", n + 1), root(), 0.0),
                        occurrence(1, "cube.ipt", root(), 0.0),
                    ],
                )
            })
            .collect();
        let named: Vec<(&str, Vec<Value>)> = chain
            .iter()
            .map(|(name, rows)| (name.as_str(), rows.clone()))
            .collect();
        let project = Project::new(&named);
        let mut report = Report::default();
        project.read("d0.iam", &mut report).unwrap();
        assert!(
            dropped(&report)
                .iter()
                .any(|loss| loss.contains("nest deeper")),
            "{:?}",
            dropped(&report)
        );
    }

    #[test]
    fn an_assembly_with_no_readable_part_is_an_error() {
        let project = Project::new(&[("lonely.iam", vec![occurrence(0, "gone.ipt", root(), 0.0)])]);
        let err = project
            .read("lonely.iam", &mut Report::default())
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("give the project folder that holds them"),
            "{err}"
        );
    }

    #[test]
    fn an_assembly_that_places_nothing_is_empty_rather_than_missing_its_parts() {
        let project = Project::new(&[("empty.iam", vec![])]);
        let mut report = Report::default();
        let model = project.read("empty.iam", &mut report).unwrap();
        let assembly = model.assembly.unwrap();
        assert!(assembly.parts.is_empty() && assembly.placements.is_empty());
        assert!(
            report
                .losses()
                .iter()
                .any(|loss| loss.severity == Severity::Info
                    && loss.detail.contains("the assembly places no parts")),
            "{report:?}"
        );
    }

    #[test]
    fn parts_in_the_folders_of_an_inventor_project_are_found() {
        // Robot/Robot.ipj, Robot/Assemblies/top.iam, Robot/Parts/cube.ipt and
        // Robot/SubAssembly/sub.iam, stored with the paths of the machine that saved them.
        let project = Project::new(&[
            (
                "top.iam",
                vec![
                    occurrence(0, r"C:\Users\someone\Robot\Parts\cube.ipt", root(), 0.0),
                    occurrence(
                        1,
                        r"C:\Users\someone\Robot\SubAssembly\sub.iam",
                        root(),
                        50.0,
                    ),
                ],
            ),
            (
                "sub.iam",
                vec![occurrence(0, r"\\server\old\cube.ipt", root(), 0.0)],
            ),
        ]);
        let top = project.dir.path().join("Robot");
        for folder in ["Assemblies", "Parts", "SubAssembly"] {
            std::fs::create_dir_all(top.join(folder)).unwrap();
        }
        std::fs::write(top.join("Robot.ipj"), b"").unwrap();
        for (from, to) in [
            ("top.iam", "Assemblies/top.iam"),
            ("Cube.IPT", "Parts/cube.ipt"),
            ("sub.iam", "SubAssembly/sub.iam"),
        ] {
            std::fs::rename(project.dir.path().join(from), top.join(to)).unwrap();
        }
        let decode =
            |format: Format, path: &Path, report: &mut Report| project.decode(format, path, report);
        let mut report = Report::default();
        let model = read(
            Format::InventorAssembly,
            &top.join("Assemblies/top.iam"),
            &mut report,
            &decode,
            &Finder::new(&PartSearch::default()),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(dropped(&report), Vec::<String>::new());
        let assembly = model.assembly.unwrap();
        assert_eq!(assembly.placements.len(), 2);
        let sub = assembly.parts[1].assembly.as_ref().unwrap();
        assert_eq!(
            sub.placements.len(),
            1,
            "the sub-assembly's cube, found by searching"
        );
        // Each part found, and the folders it was found in, are names the report met: by
        // name, as every system spells the path to them its own way (`\\`, `/private/var`).
        for name in ["Robot", "Parts", "SubAssembly", "cube.ipt", "cube"] {
            assert!(
                report.names().contains(name),
                "{name}: {:?}",
                report.names()
            );
        }
        assert!(
            report
                .names()
                .iter()
                .any(|name| name.ends_with("cube.ipt") && name.contains("Parts")),
            "the whole path found: {:?}",
            report.names()
        );
    }

    /// cadmpeg's example 10 mm cube in a design whose component `Block` owns it, placed at
    /// the origin and 30 mm along X; with `owned` false the component does not say what it owns.
    fn with_components(owned: bool) -> Model {
        let part = crate::read(
            Format::InventorPart,
            &fixture("synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        let mut wire = serde_json::to_value(&part.ir).unwrap();
        let body = wire["model"]["bodies"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let bodies = if owned { json!([body]) } else { json!([]) };
        wire["model"]["product_definitions"] = json!([{
            "id": "f3d:model:component#block", "kind": "part", "label": "Block", "bodies": bodies,
        }]);
        let placed = |key: u32, x: f64, parent: Value| {
            json!({
                "id": format!("f3d:model:occurrence#{key}"),
                "prototype": {"scope": "local", "definition": "f3d:model:component#block"},
                "parent": parent,
                "ordinal": key,
                "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
                "scale": [1.0, 1.0, 1.0],
            })
        };
        wire["model"]["occurrences"] = json!([placed(0, 0.0, root()), placed(1, 30.0, root())]);
        Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..part
        }
    }

    #[test]
    fn components_that_own_their_bodies_become_an_assembly() {
        let assembly = local_assembly(&with_components(true)).unwrap().unwrap();
        assert_eq!(assembly.parts.len(), 1);
        let part = &assembly.parts[0];
        assert_eq!(part.name, "Block");
        assert_eq!(part.ir.model.bodies.len(), 1);
        assert_eq!(part.ir.model.faces.len(), 6);
        assert!(part.ir.model.occurrences.is_empty());
        assert_eq!(assembly.placements.len(), 2);
        assert_eq!(assembly.placements[1].transform[0][3], 30.0);
    }

    #[test]
    fn components_that_do_not_say_what_they_own_convert_as_stored() {
        let err = local_assembly(&with_components(false)).unwrap_err();
        assert_eq!(
            err,
            "cadmpeg does not say which bodies belong to component Block"
        );
    }

    #[test]
    fn a_component_that_is_empty_in_the_source_is_placed_empty() {
        // Every body belongs to `Block`, so `Carriage`, naming none and holding none, is
        // empty rather than unattributed.
        let model = with_components(true);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["product_definitions"]
            .as_array_mut()
            .unwrap()
            .push(
                json!({"id": "f3d:model:component#carriage", "kind": "part", "label": "Carriage"}),
            );
        wire["model"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": "f3d:model:occurrence#2",
                "prototype": {"scope": "local", "definition": "f3d:model:component#carriage"},
                "parent": root(), "ordinal": 2,
                "transform": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
                "scale": [1.0, 1.0, 1.0],
            }));
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model
        };
        let assembly = local_assembly(&model).unwrap().unwrap();
        assert_eq!(assembly.parts.len(), 2);
        assert_eq!(assembly.parts[1].name, "Carriage");
        assert!(assembly.parts[1].ir.model.bodies.is_empty());
        assert_eq!(assembly.placements.len(), 3);
    }

    /// cadmpeg's example cube, owned by component `Block`, placed twice inside a `Frame`
    /// placed 100 mm along X. `frame_owns` gives the frame the cube too; `rows` replaces the
    /// occurrences.
    fn nested(frame_owns: bool, rows: Option<Value>) -> Model {
        let part = crate::read(
            Format::InventorPart,
            &fixture("synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        let mut wire = serde_json::to_value(&part.ir).unwrap();
        let body = wire["model"]["bodies"][0]["id"].clone();
        let frame_bodies = if frame_owns { json!([body]) } else { json!([]) };
        wire["model"]["product_definitions"] = json!([
            {"id": "f3d:model:component#frame", "kind": "part", "label": "Frame", "bodies": frame_bodies},
            {"id": "f3d:model:component#block", "kind": "part", "label": "Block", "bodies": [body]},
        ]);
        wire["model"]["occurrences"] = rows.unwrap_or_else(|| {
            json!([
                placed(0, "frame", root(), 100.0),
                placed(1, "block", inside(0), 0.0),
                placed(2, "block", inside(0), 30.0),
            ])
        });
        Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..part
        }
    }

    /// `model` with these assembly joints.
    fn with_joints(model: Model, joints: Value) -> Model {
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["assembly_joints"] = joints;
        Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model
        }
    }

    /// One side of a joint: the design's root, or an occurrence (with nested occurrence keys).
    fn connector(occurrence: Option<(u32, &[&str])>) -> Value {
        let operand = match occurrence {
            None => json!({"container": {"container": "root"}, "object": "root:origin"}),
            Some((key, nested)) => json!({
                "container": {"container": "occurrence", "occurrence": format!("f3d:model:occurrence#{key}")},
                "object": key.to_string(),
                "subelements": nested,
            }),
        };
        json!({"operand": operand, "frame": IDENTITY, "detached": false})
    }

    /// A connector on occurrence `key` (with nested occurrence keys) with its frame `frame`.
    fn connector_at(key: u32, nested: &[&str], frame: Placement3x4) -> Value {
        let mut out = connector(Some((key, nested)));
        out["frame"] = json!(frame);
        out
    }

    /// A placement at `origin`, turned by `turn` radians about z.
    fn at(origin: [f64; 3], turn: f64) -> Placement3x4 {
        let (s, c) = turn.sin_cos();
        [
            [c, -s, 0.0, origin[0]],
            [s, c, 0.0, origin[1]],
            [0.0, 0.0, 1.0, origin[2]],
        ]
    }

    /// `frame` turned half a turn about its x axis, as a joint's frame faces the other.
    fn facing(frame: Placement3x4) -> Placement3x4 {
        compose(&frame, &FACING)
    }

    /// Block 2's placement in the world (and the joint, and the notes) once one joint `kind`
    /// joins block 2 (stored at x = 130) to block 1 (placed at x = 100 in the world) with
    /// these connectors, block 2's first.
    fn joined(kind: Value, a: Value, b: Value) -> (Placement3x4, Joint, Vec<String>) {
        let model = with_joints(nested(false, None), json!([joint(1, kind, a, b)]));
        let (assembly, notes) = local_components(&model).unwrap().unwrap();
        let world = world_at(&assembly, &[0, 1]).unwrap();
        (world, assembly.joints[0].clone(), notes)
    }

    fn near(a: &Placement3x4, b: &Placement3x4) -> bool {
        same(a, b)
    }

    #[test]
    fn a_joint_from_an_occurrence_to_one_inside_it_moves_the_inner_one() {
        // The frame (stored at x = 100) joined rigidly to block 2 inside it, the frame's side
        // first, as a rigid group holds its members to its first: the frame keeps its stored
        // placement and block 2 moves to meet it. Moving the frame would move block 2 with it.
        let row = joint(
            1,
            json!({"kind": "fixed"}),
            connector_at(0, &[], at([5.0, 0.0, 0.0], 0.0)),
            connector_at(2, &[], at([0.0, 0.0, 0.0], 0.0)),
        );
        let model = with_joints(nested(false, None), json!([row]));
        let (assembly, _) = local_components(&model).unwrap().unwrap();
        let joint = &assembly.joints[0];
        assert_eq!((joint.a.clone(), joint.b.clone()), (vec![0], vec![0, 1]));
        let frame = world_at(&assembly, &[0]).unwrap();
        assert!(near(&frame, &at([100.0, 0.0, 0.0], 0.0)), "{frame:?}");
        let block = world_at(&assembly, &[0, 1]).unwrap();
        assert!(near(
            &compose(&block, &joint.frame_b),
            &compose(&compose(&frame, &joint.frame_a), &joint.meeting())
        ));
    }

    #[test]
    fn an_aligned_joint_meets_its_frames_z_to_z() {
        // The same rigid joint with both frames pointing up: face to face, block 2 is turned
        // over to meet block 1; aligned, it is not turned.
        let placed_with = |flip: bool| {
            let row = joint(
                1,
                json!({"kind": "fixed"}),
                connector_at(2, &[], at([0.0, 0.0, 0.0], 0.0)),
                connector_at(1, &[], at([10.0, 0.0, 0.0], 0.0)),
            );
            // The joint as the file gives it, then the parts placed by it with each sense.
            let model = with_joints(nested(false, None), json!([row]));
            let mut joint = local_components(&model).unwrap().unwrap().0.joints[0].clone();
            joint.flip = flip;
            let mut fresh = local_components(&nested(false, None)).unwrap().unwrap().0;
            fresh.joints = vec![joint];
            let notes = place_by_joints(&mut fresh);
            assert!(notes.is_empty(), "{notes:?}");
            world_at(&fresh, &[0, 1]).unwrap()
        };
        assert!(near(&placed_with(true), &at([110.0, 0.0, 0.0], 0.0)));
        assert!(near(
            &placed_with(false),
            &compose(&at([110.0, 0.0, 0.0], 0.0), &FACING)
        ));
    }

    #[test]
    fn a_joint_takes_its_flip_from_its_operands_frame_sense() {
        let flip = |sense: Option<&str>| {
            let mut row = joint(
                1,
                json!({"kind": "fixed"}),
                connector_at(2, &[], at([0.0, 0.0, 0.0], 0.0)),
                connector_at(1, &[], at([10.0, 0.0, 0.0], 0.0)),
            );
            if let Some(sense) = sense {
                row["operands"]["frame_sense"] = json!(sense);
            }
            let model = with_joints(nested(false, None), json!([row]));
            local_components(&model).unwrap().unwrap().0.joints[0].flip
        };
        assert!(flip(Some("aligned")));
        assert!(!flip(Some("opposed")));
        assert!(!flip(None));
    }

    #[test]
    fn a_joint_left_out_is_noted_with_its_kind_for_the_writer() {
        let row = joint(
            1,
            json!({"kind": "cylindrical"}),
            connector_at(2, &[], IDENTITY),
            connector_at(1, &[], IDENTITY),
        );
        let model = with_joints(nested(false, None), json!([row]));
        let (assembly, notes) = local_components(&model).unwrap().unwrap();
        assert!(assembly.joints.is_empty());
        assert_eq!(
            assembly.joint_notes,
            [JointNote {
                text: "joint 1: cylindrical joints are not converted yet".to_owned(),
                family: Some("cylindrical".to_owned()),
            }]
        );
        assert_eq!(notes, ["joint 1: cylindrical joints are not converted yet"]);
    }

    #[test]
    fn a_joint_is_aligned_only_when_its_source_says_so() {
        let sensed = |sense: &str| json!({"operands": {"arity": "pair", "frame_sense": sense}});
        assert!(frame_aligned(&sensed("aligned")));
        assert!(!frame_aligned(&sensed("opposed")));
        assert!(!frame_aligned(&json!({"operands": {"arity": "pair"}})));
        // Only the operands carry the sense: a stray top-level field is not read.
        assert!(!frame_aligned(&json!({"frame_sense": "aligned"})));
    }

    #[test]
    fn a_joint_moves_its_first_part_to_where_its_frames_meet() {
        // A hinge at block 2's own origin and 10 mm past block 1's, facing it: whatever block
        // 2's stored placement, the joint puts it at x = 110, and block 1 stays.
        let hinge = || {
            (
                connector_at(2, &[], at([0.0, 0.0, 0.0], 0.0)),
                connector_at(1, &[], facing(at([10.0, 0.0, 0.0], 0.0))),
            )
        };
        let (a, b) = hinge();
        let (world, joint, notes) = joined(json!({"kind": "revolute"}), a, b);
        assert!(notes.is_empty(), "{notes:?}");
        assert!(near(&world, &at([110.0, 0.0, 0.0], 0.0)), "{world:?}");
        assert_eq!(joint.frame_a, at([0.0, 0.0, 0.0], 0.0));
        // Turned half a radian, the second frame turns half a radian from the first, so block
        // 2 turns back by as much about the hinge.
        let (a, b) = hinge();
        let (world, _, _) = joined(json!({"kind": "revolute", "angle": 0.5}), a, b);
        assert!(near(&world, &at([110.0, 0.0, 0.0], -0.5)), "{world:?}");
        // A slider 7 mm out along the first frame's z, which faces down once they meet.
        let (a, b) = hinge();
        let (world, _, _) = joined(json!({"kind": "slider", "distance": 7.0}), a, b);
        assert!(near(&world, &at([110.0, 0.0, -7.0], 0.0)), "{world:?}");
        // A fixed joint carries its offset in the first frame: 5 mm along it, then a quarter
        // turn about it.
        let (a, b) = hinge();
        let (world, _, _) = joined(
            json!({"kind": "fixed", "angle": std::f64::consts::FRAC_PI_2,
                   "translation_offset": [5.0, 0.0, 0.0]}),
            a,
            b,
        );
        assert!(
            near(&world, &at([110.0, 5.0, 0.0], -std::f64::consts::FRAC_PI_2)),
            "{world:?}"
        );
    }

    #[test]
    fn a_part_joined_by_its_face_stands_on_the_face_it_is_joined_to() {
        // As Fusion stores a pin jointed onto a block: the pin's frame on its bottom face, z
        // out of the face (down), and the block's on its top face 10 mm up, z up. The pin
        // stands on the block, upright, whatever its stored placement.
        let (world, _, notes) = joined(
            json!({"kind": "fixed"}),
            connector_at(2, &[], facing(IDENTITY)),
            connector_at(1, &[], at([0.0, 0.0, 10.0], 0.0)),
        );
        assert!(notes.is_empty(), "{notes:?}");
        assert!(near(&world, &at([100.0, 0.0, 10.0], 0.0)), "{world:?}");
    }

    #[test]
    fn a_frame_named_through_its_container_is_moved_into_the_part_it_is_on() {
        // Block 2's frame, given in the Frame occurrence's space (which places block 2 at
        // x = 30 in it): 30 mm in, it is block 2's own origin.
        let (world, joint, _) = joined(
            json!({"kind": "revolute"}),
            connector_at(0, &["2"], at([30.0, 0.0, 0.0], 0.0)),
            connector_at(1, &[], facing(at([10.0, 0.0, 0.0], 0.0))),
        );
        assert!(near(&joint.frame_a, &IDENTITY), "{:?}", joint.frame_a);
        assert!(near(&world, &at([110.0, 0.0, 0.0], 0.0)), "{world:?}");
    }

    #[test]
    fn revolute_limits_are_kept_and_a_one_sided_one_is_left_out() {
        let (_, joint, notes) = joined(
            json!({"kind": "revolute", "angular_limits": {"bounds": "range", "minimum": -1.0, "maximum": 1.0}}),
            connector_at(1, &[], IDENTITY),
            connector_at(2, &[], IDENTITY),
        );
        assert!(notes.is_empty());
        assert_eq!(
            joint.motion,
            Motion::Revolute {
                angle: 0.0,
                limits: Some((-1.0, 1.0))
            }
        );
        let (_, joint, notes) = joined(
            json!({"kind": "revolute", "angular_limits": {"bounds": "minimum", "minimum": -1.0}}),
            connector_at(1, &[], IDENTITY),
            connector_at(2, &[], IDENTITY),
        );
        assert!(matches!(
            joint.motion,
            Motion::Revolute { limits: None, .. }
        ));
        assert_eq!(notes, ["joint 1: its limit on one side only is left out"]);
    }

    #[test]
    fn joints_that_place_a_part_two_ways_are_reported() {
        let rigid = json!({"kind": "fixed"});
        let model = with_joints(
            nested(false, None),
            json!([
                joint(
                    1,
                    rigid.clone(),
                    connector_at(1, &[], IDENTITY),
                    connector_at(2, &[], IDENTITY)
                ),
                joint(
                    2,
                    rigid,
                    connector_at(1, &[], IDENTITY),
                    connector_at(2, &[], at([1.0, 0.0, 0.0], 0.0))
                ),
            ]),
        );
        let (_, notes) = local_components(&model).unwrap().unwrap();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("joint 2: its parts are placed by other joints"));
    }

    fn joint(key: u32, kind: Value, a: Value, b: Value) -> Value {
        json!({
            "id": format!("f3d:model:joint#{key}"),
            "operands": {"arity": "pair", "kind": kind, "connectors": [a, b]},
            "suppressed": false,
        })
    }

    /// Block 2 stored 10 mm above block 1, turned 0.3 rad about z, both inside the frame at
    /// x = 100, and joined by one joint `kind`: a frame on block 2's origin facing down, and
    /// one on block 1's facing up. Block 2's placement in the world, the joint and the notes.
    fn on_its_axis(kind: Value) -> (Placement3x4, Joint, Vec<String>) {
        let mut turned = placed(2, "block", inside(0), 0.0);
        turned["transform"] = json!(at([0.0, 0.0, 10.0], 0.3));
        let rows = json!([
            placed(0, "frame", root(), 100.0),
            placed(1, "block", inside(0), 0.0),
            turned
        ]);
        let model = with_joints(
            nested(false, Some(rows)),
            json!([joint(
                1,
                kind,
                connector_at(2, &[], facing(IDENTITY)),
                connector_at(1, &[], IDENTITY)
            )]),
        );
        let (assembly, notes) = local_components(&model).unwrap().unwrap();
        let world = world_at(&assembly, &[0, 1]).unwrap();
        let joint = assembly.joints[0].clone();
        // Wherever it leaves them, the joint holds its parts where they are.
        let a = compose(&world, &joint.frame_a);
        let b = compose(&world_at(&assembly, &[0, 0]).unwrap(), &joint.frame_b);
        assert!(near(&compose(&a, &joint.meeting()), &b), "{joint:?}");
        (world, joint, notes)
    }

    #[test]
    fn a_revolute_whose_parts_turn_on_its_axis_holds_them_as_built() {
        // The parts are placed 0.3 rad apart about the axis; the source's angle of 0.5 rad
        // aligns the frames, so the joint has turned -0.2 rad from there. They stay as placed.
        let (world, joint, notes) = on_its_axis(json!({"kind": "revolute", "angle": 0.5,
            "angular_limits": {"bounds": "range", "minimum": -1.0, "maximum": 1.0}}));
        assert!(near(&world, &at([100.0, 0.0, 10.0], 0.3)), "{world:?}");
        let Motion::Revolute { angle, limits } = joint.motion else {
            panic!("{:?}", joint.motion);
        };
        assert!((angle + 0.2).abs() < 1e-9, "{angle}");
        assert_eq!(limits, Some((-1.0, 1.0)));
        assert!(notes.is_empty(), "{notes:?}");
        // The limits hold from the aligned frames: -0.2 rad is inside these.
        let (_, joint, notes) = on_its_axis(json!({"kind": "revolute", "angle": 0.5,
            "angular_limits": {"bounds": "range", "minimum": -0.25, "maximum": 0.0}}));
        assert!(matches!(
            joint.motion,
            Motion::Revolute {
                limits: Some(_),
                ..
            }
        ));
        assert!(notes.is_empty(), "{notes:?}");
        // Limits the angle lies outside are left out, with the reason.
        let (_, joint, notes) = on_its_axis(json!({"kind": "revolute", "angle": 0.0,
            "angular_limits": {"bounds": "range", "minimum": -0.2, "maximum": 0.2}}));
        assert!(matches!(
            joint.motion,
            Motion::Revolute { limits: None, .. }
        ));
        assert_eq!(
            notes,
            ["joint 1: its limits are left out: its angle lies outside them"]
        );
    }

    #[test]
    fn a_fixed_joint_its_parts_already_meet_either_way_holds_them_as_built() {
        // Block 2 stored where block 1 is, their joint frames coinciding with z the same way:
        // Fusion mated them flipped. They stay; the joint is not turned over to mate them.
        let rows = |turned: f64| {
            let mut second = placed(2, "block", inside(0), 0.0);
            second["transform"] = json!(at([0.0, 0.0, 0.0], turned));
            json!([
                placed(0, "frame", root(), 100.0),
                placed(1, "block", inside(0), 0.0),
                second
            ])
        };
        let joined = |turned: f64| {
            let model = with_joints(
                nested(false, Some(rows(turned))),
                json!([joint(
                    1,
                    json!({"kind": "fixed"}),
                    connector_at(2, &[], IDENTITY),
                    connector_at(1, &[], IDENTITY)
                )]),
            );
            let (assembly, notes) = local_components(&model).unwrap().unwrap();
            assert!(notes.is_empty(), "{notes:?}");
            let world = world_at(&assembly, &[0, 1]).unwrap();
            let joint = assembly.joints[0].clone();
            let a = compose(&world, &joint.frame_a);
            let b = compose(&world_at(&assembly, &[0, 0]).unwrap(), &joint.frame_b);
            assert!(near(&compose(&a, &joint.meeting()), &b), "{joint:?}");
            world
        };
        assert!(near(&joined(0.0), &at([100.0, 0.0, 0.0], 0.0)));
        // Turned about the axis, the parts do not meet as the joint says: it places them.
        assert!(!near(&joined(0.3), &at([100.0, 0.0, 0.0], 0.3)));
    }

    #[test]
    fn a_slider_whose_parts_lie_on_its_axis_holds_them_as_built() {
        let (world, joint, notes) = on_its_axis(json!({"kind": "slider", "distance": 2.0}));
        assert!(near(&world, &at([100.0, 0.0, 10.0], 0.3)), "{world:?}");
        let Motion::Slider { distance } = joint.motion else {
            panic!("{:?}", joint.motion);
        };
        assert!((distance - 10.0).abs() < 1e-9, "{distance}");
        assert!(notes.is_empty(), "{notes:?}");
    }

    /// A block placed at the origin, and a design inserted into this one (as an .f3z carries
    /// it, named `Gripper`) holding a block 5 mm along x, joined by `joints`.
    fn with_inserted_design(joints: Value) -> Model {
        let model = nested(false, Some(json!([])));
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let mut finger = placed(9, "block", json!({}), 5.0);
        finger["id"] = json!("f3d:xref/role-1/model:occurrence#finger");
        finger["parent"] = inside(8);
        wire["model"]["occurrences"] = json!([
            placed(0, "block", root(), 0.0),
            {
                "id": "f3d:model:occurrence#8",
                "prototype": {"scope": "external",
                    "document": {"resolution": "path", "path": "gripper.f3d"}},
                "parent": root(),
                "ordinal": 1,
                "transform": IDENTITY,
                "scale": [1.0, 1.0, 1.0],
            },
            finger,
        ]);
        wire["model"]["assembly_joints"] = joints;
        wire["native"]["f3d"]["xref_designs"] = json!([
            {"id": "f3d:xref:design#0", "display_name": "Gripper",
             "target_file_name": "gripper.f3d"},
        ]);
        Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model
        }
    }

    /// A connector on the component keyed `key` inside a design inserted into this one.
    fn inserted_connector(key: &str) -> Value {
        json!({"operand": {"container": {"container": "external",
                "external_document": {"resolution": "document_id", "document_id": "d"}},
              "object": "o", "subelements": [key]},
            "frame": IDENTITY, "detached": false})
    }

    #[test]
    fn a_design_inserted_into_this_one_is_placed_and_joined_as_a_whole() {
        // The joint's frame on the gripper is the finger's own, 5 mm along x inside it: it
        // faces the block's 50 mm up, and the whole gripper stands so the finger's frame is
        // there.
        let model = with_inserted_design(json!([joint(
            1,
            json!({"kind": "fixed"}),
            inserted_connector("finger"),
            connector_at(0, &[], at([0.0, 0.0, 50.0], 0.0)),
        )]));
        let (assembly, notes) = local_components(&model).unwrap().unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        let gripper = assembly
            .placements
            .iter()
            .position(|p| assembly.parts[p.part].name == "Gripper")
            .unwrap();
        let inner = assembly.parts[assembly.placements[gripper].part]
            .assembly
            .as_ref()
            .unwrap();
        assert_eq!(inner.placements.len(), 1);
        assert_eq!(inner.placements[0].transform[0][3], 5.0);
        assert_eq!(assembly.joints[0].a, [gripper]);
        let world = world_at(&assembly, &[gripper]).unwrap();
        let finger = at([5.0, 0.0, 0.0], 0.0);
        assert!(
            near(
                &compose(&world, &finger),
                &facing(at([0.0, 0.0, 50.0], 0.0))
            ),
            "{world:?}"
        );
        // A key no component inside it has joins nothing.
        let model = with_inserted_design(json!([joint(
            1,
            json!({"kind": "fixed"}),
            inserted_connector("thumb"),
            connector_at(0, &[], IDENTITY),
        )]));
        let (assembly, notes) = local_components(&model).unwrap().unwrap();
        assert!(assembly.joints.is_empty());
        assert_eq!(notes, ["joint 1: it joins a component of another design"]);
    }

    #[test]
    fn fixed_joints_join_the_placements_they_name() {
        let fixed = json!({"kind": "fixed", "angle": 0.0, "translation_offset": [0.0, 0.0, 0.0]});
        let model = with_joints(
            nested(false, None),
            json!([
                // Between the two blocks inside the frame.
                joint(
                    1,
                    fixed.clone(),
                    connector(Some((1, &[]))),
                    connector(Some((2, &[])))
                ),
                // The frame to the design itself.
                joint(2, fixed.clone(), connector(None), connector(Some((0, &[])))),
                // The second block, named through the frame it sits in: its frame is in the
                // frame's space, where the block is stored 30 mm along X.
                joint(
                    3,
                    fixed.clone(),
                    connector(Some((1, &[]))),
                    connector_at(0, &["2"], at([30.0, 0.0, 0.0], 0.0))
                ),
            ]),
        );
        let (assembly, left_out) = local_components(&model).unwrap().unwrap();
        assert!(left_out.is_empty(), "{left_out:?}");
        let sides: Vec<(Vec<usize>, Vec<usize>)> = assembly
            .joints
            .iter()
            .map(|j| (j.a.clone(), j.b.clone()))
            .collect();
        assert_eq!(
            sides,
            [
                (vec![0, 0], vec![0, 1]),
                (vec![], vec![0]),
                (vec![0, 0], vec![0, 1])
            ]
        );
    }

    #[test]
    fn a_grounded_nested_occurrence_keeps_its_world_placement() {
        let model = with_joints(
            nested(false, None),
            json!([{
                "id": "f3d:model:joint#1",
                "operands": {
                    "arity": "grounded",
                    "connector": connector_at(0, &["2"], at([8.0, 9.0, 10.0], 0.5)),
                    "offset_frame": at([1.0, 2.0, 3.0], 0.25),
                },
                "suppressed": false,
            }]),
        );
        let (assembly, notes) = local_components(&model).unwrap().unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(assembly.joints.len(), 1);
        let ground = &assembly.joints[0];
        assert!(ground.a.is_empty());
        assert_eq!(ground.b, [0, 1]);
        assert_eq!(ground.motion, Motion::Rigid);
        let world = at([130.0, 0.0, 0.0], 0.0);
        assert_eq!(world_at(&assembly, &ground.b), Some(world));
        assert_eq!(ground.frame_a, world);
        assert_eq!(ground.frame_b, FACING);
    }

    #[test]
    fn joints_that_cannot_be_written_are_left_out_with_the_reason() {
        let fixed = json!({"kind": "fixed"});
        let model = with_joints(
            nested(false, None),
            json!([
                joint(
                    1,
                    json!({"kind": "ball"}),
                    connector(Some((1, &[]))),
                    connector(Some((2, &[])))
                ),
                joint(
                    2,
                    fixed.clone(),
                    connector(Some((1, &[]))),
                    connector(Some((1, &[])))
                ),
                joint(
                    3,
                    fixed.clone(),
                    connector(Some((1, &[]))),
                    connector(Some((9, &[])))
                ),
                joint(
                    4,
                    fixed.clone(),
                    connector(Some((1, &[]))),
                    json!({"operand": {"container": {"container": "external",
                        "external_document": {"resolution": "missing"}}, "object": "x"},
                        "frame": IDENTITY, "detached": false})
                ),
            ]),
        );
        let (assembly, left_out) = local_components(&model).unwrap().unwrap();
        assert!(assembly.joints.is_empty());
        assert_eq!(
            left_out,
            [
                "joint 1: ball joints are not converted yet",
                "joint 2: it joins a component to itself",
                "joint 3: it joins a component the design does not place",
                "joint 4: it joins a component of another design",
            ]
        );
    }

    #[test]
    fn a_component_holding_others_converts_wherever_it_is_placed() {
        // Two frames, each holding the two blocks in the same places.
        let model = nested(
            false,
            Some(json!([
                placed(0, "frame", root(), 0.0),
                placed(1, "frame", root(), 100.0),
                placed(2, "block", inside(0), 0.0),
                placed(3, "block", inside(0), 30.0),
                placed(4, "block", inside(1), 0.0),
                placed(5, "block", inside(1), 30.0),
            ])),
        );
        let fixed = json!({"kind": "fixed"});
        let model = with_joints(
            model,
            json!([
                // The frame's own joint, stored once for each frame.
                joint(
                    1,
                    fixed.clone(),
                    connector(Some((4, &[]))),
                    connector(Some((5, &[])))
                ),
                joint(
                    2,
                    fixed.clone(),
                    connector(Some((2, &[]))),
                    connector(Some((3, &[])))
                ),
                // From one frame's block to the other frame's, and to the design.
                joint(
                    3,
                    fixed.clone(),
                    connector(Some((2, &[]))),
                    connector(Some((4, &[])))
                ),
                joint(4, fixed, connector(None), connector(Some((3, &[])))),
            ]),
        );
        let (assembly, left_out) = local_components(&model).unwrap().unwrap();
        assert_eq!(assembly.parts.len(), 1, "one frame, placed twice");
        assert_eq!(assembly.placements.len(), 2);
        // The frame's joint is written once, through the first frame: the design holds the
        // frame once, so both frames' blocks follow it.
        let sides: Vec<_> = assembly
            .joints
            .iter()
            .map(|j| (j.a.clone(), j.b.clone()))
            .collect();
        // A joint from one frame's block to something outside that frame cannot hold the block
        // alone, as both frames hold it: it holds that whole frame.
        assert_eq!(
            sides,
            [
                (vec![0, 0], vec![0, 1]),
                (vec![0], vec![1]),
                (vec![], vec![0]),
            ]
        );
        assert_eq!(
            left_out,
            [
                format!("joint 3: {WHOLE_COPY}"),
                format!("joint 4: {WHOLE_COPY}"),
            ]
        );
        // The second frame is placed so that its first block meets the first frame's.
        let first = world_at(&assembly, &[0, 0]).unwrap();
        let second = world_at(&assembly, &[1, 0]).unwrap();
        for axis in 0..3 {
            assert!(
                (first[axis][3] - second[axis][3]).abs() < 1e-9,
                "{first:?} {second:?}"
            );
        }
    }

    #[test]
    fn a_nested_occurrence_named_by_its_key_alone_is_found_inside_its_container() {
        // As inside a design inserted into the frame: the block's id ends in its key but is
        // not built from the frame's.
        let mut block = placed(1, "block", inside(0), 0.0);
        block["id"] = json!("f3d:inserted/model:occurrence#a1");
        let model = nested(
            false,
            Some(json!([
                placed(0, "frame", root(), 100.0),
                block,
                placed(2, "block", inside(0), 30.0),
            ])),
        );
        let model = with_joints(
            model,
            json!([joint(
                1,
                json!({"kind": "fixed"}),
                connector(Some((0, &["a1"]))),
                connector(Some((2, &[])))
            )]),
        );
        let (assembly, left_out) = local_components(&model).unwrap().unwrap();
        assert!(left_out.is_empty(), "{left_out:?}");
        let sides: Vec<_> = assembly
            .joints
            .iter()
            .map(|j| (j.a.clone(), j.b.clone()))
            .collect();
        assert_eq!(sides, [(vec![0, 0], vec![0, 1])]);
    }

    fn placed(key: u32, definition: &str, parent: Value, x: f64) -> Value {
        json!({
            "id": format!("f3d:model:occurrence#{key}"),
            "prototype": {"scope": "local", "definition": format!("f3d:model:component#{definition}")},
            "parent": parent,
            "ordinal": key,
            "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
            "scale": [1.0, 1.0, 1.0],
        })
    }

    fn inside(key: u32) -> Value {
        json!({"kind": "occurrence", "occurrence": format!("f3d:model:occurrence#{key}")})
    }

    #[test]
    fn a_designs_root_component_is_held_where_it_is() {
        // The frame is the document's root component, and the first block the root of a
        // design inserted inside it: both are held; the second block is not.
        let mut frame = placed(0, "frame", root(), 100.0);
        frame["id"] = json!("f3d:model:occurrence#root-0");
        let inside_root =
            json!({"kind": "occurrence", "occurrence": "f3d:model:occurrence#root-0"});
        let mut first = placed(1, "block", inside_root.clone(), 0.0);
        first["id"] = json!("f3d:model:occurrence#root-1");
        let second = placed(2, "block", inside_root, 30.0);
        let assembly = local_assembly(&nested(false, Some(json!([frame, first, second]))))
            .unwrap()
            .unwrap();
        let mut held = assembly.held.clone();
        held.sort();
        assert_eq!(held, [vec![0], vec![0, 0]]);
    }

    #[test]
    fn components_inside_components_become_sub_assemblies() {
        let assembly = local_assembly(&nested(false, None)).unwrap().unwrap();
        assert_eq!(assembly.parts.len(), 1);
        assert_eq!(assembly.placements.len(), 1);
        assert_eq!(assembly.placements[0].transform[0][3], 100.0);
        let frame = &assembly.parts[0];
        assert_eq!(frame.name, "Frame");
        assert!(frame.ir.model.bodies.is_empty());
        let inner = frame.assembly.as_ref().unwrap();
        assert_eq!(inner.parts.len(), 1, "Block is read once");
        assert_eq!(inner.parts[0].name, "Block");
        assert_eq!(inner.parts[0].ir.model.bodies.len(), 1);
        let xs: Vec<f64> = inner.placements.iter().map(|p| p.transform[0][3]).collect();
        assert_eq!(xs, [0.0, 30.0]);
    }

    #[test]
    fn a_component_holding_others_keeps_its_own_bodies_beside_them() {
        let assembly = local_assembly(&nested(true, None)).unwrap().unwrap();
        let inner = assembly.parts[0].assembly.as_ref().unwrap();
        assert_eq!(inner.parts.len(), 2);
        assert_eq!(inner.parts[1].name, "Frame bodies");
        assert_eq!(inner.placements.len(), 3);
        assert_eq!(inner.placements[2].part, 1);
        assert_eq!(inner.placements[2].transform, IDENTITY);
    }

    #[test]
    fn nested_components_that_do_not_hold_together_convert_as_stored() {
        // Two frames, holding blocks in different places.
        let err = local_assembly(&nested(
            false,
            Some(json!([
                placed(0, "frame", root(), 0.0),
                placed(1, "frame", root(), 100.0),
                placed(2, "block", inside(0), 0.0),
                placed(3, "block", inside(1), 30.0),
            ])),
        ))
        .unwrap_err();
        assert_eq!(
            err,
            "one component holds different components where it is placed"
        );
        // A block placed inside an occurrence the design does not have.
        let err = local_assembly(&nested(
            false,
            Some(json!([
                placed(0, "block", root(), 0.0),
                placed(1, "block", inside(7), 0.0),
            ])),
        ))
        .unwrap_err();
        assert_eq!(
            err,
            "some components are placed inside components the design does not place"
        );
    }

    #[test]
    fn a_design_without_components_is_not_an_assembly() {
        let part = crate::read(
            Format::InventorPart,
            &fixture("synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        assert!(local_assembly(&part).unwrap().is_none());
    }

    #[test]
    fn fusion_components_without_bodies_are_reported() {
        let mut report = Report::default();
        let model = crate::read(
            Format::FusionArchive,
            &fixture("design_copy_paste.f3d"),
            &mut report,
        )
        .unwrap();
        assert!(model.assembly.is_none());
        let loss = report
            .losses()
            .iter()
            .find(|loss| loss.subject == "components")
            .unwrap();
        assert_eq!(loss.severity, Severity::Degraded);
        assert!(
            loss.detail
                .contains("cadmpeg does not say which bodies belong to component"),
            "{}",
            loss.detail
        );
    }

    #[test]
    fn file_names_come_from_either_separator() {
        assert_eq!(file_name(r"C:\Work\Parts\Bracket.ipt"), "Bracket.ipt");
        assert_eq!(file_name("/home/a/Bracket.ipt"), "Bracket.ipt");
        assert_eq!(file_name("Bracket.ipt"), "Bracket.ipt");
    }

    #[test]
    fn placements_must_be_rigid_and_whole() {
        let identity = serde_json::json!([[1, 0, 0, 5], [0, 1, 0, 0], [0, 0, 1, 0]]);
        let row =
            |scale: serde_json::Value| serde_json::json!({"transform": identity, "scale": scale});
        assert_eq!(
            transform(&row(serde_json::json!([1, 1, 1]))).unwrap()[0],
            [1.0, 0.0, 0.0, 5.0]
        );
        assert!(transform(&row(serde_json::json!([2, 1, 1]))).is_err());
        assert!(transform(&serde_json::json!({"transform": [[1, 0, 0]]})).is_err());
    }
}
