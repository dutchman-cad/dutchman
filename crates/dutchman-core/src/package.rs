//! Reads a placed-parts package (`dutchman.parts/1`, dutchman-cad/dutchman#1005) into a
//! [`Model`].
//!
//! A package is a folder of STEP files and a manifest an optional module prints:
//!
//! - `models`: each geometry file once, `{id, file}` (a STEP file named relative to the folder),
//! - `parts`: placed instances, `{name, model, transform, notes}`; `name` is the stable
//!   identity, `transform` a 3 x 4 matrix (rotation rows, translation in the last column),
//! - `overlays`: flat graphics on a part (copper, silkscreen) as finished polygons,
//! - `units`, `name` and `notes`.
//!
//! Every STEP file goes through the one guarded decode path ([`crate::read_bytes`]). The parts
//! become [`Model::assembly`] the way a design's own components do: one part per model, one
//! placement per package part, so a model used a hundred times is one definition. The package
//! is untrusted: its size, its counts and its file names are checked before anything is read,
//! and a file outside the package folder refuses the whole package.
//!
//! An overlay is one sheet body named after it, one planar face per area (outer loop plus hole
//! loops) at the overlay's height in its part's own frame, placed with that part's transform and
//! coloured. The face is written as STEP text and read back through the same decode path, so
//! the writer treats it like any other STEP part.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::placement::{Placement3x4, IDENTITY};
use crate::{
    Assembly, Error, Format, LeftOut, Model, Placement, Report, Severity, Stage, MAX_INPUT_BYTES,
    MAX_PARTS, MAX_PLACEMENTS,
};

/// The manifest schema this reader takes.
pub const PARTS_SCHEMA: &str = "dutchman.parts/1";
/// The largest manifest read, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 16 << 20;
/// The most STEP bytes one package may bring in, counting each model file once.
pub const MAX_PACKAGE_BYTES: u64 = 4 << 30;
/// The most overlays one package may carry.
pub const MAX_OVERLAYS: usize = 16;
/// The most areas one overlay may carry.
pub const MAX_AREAS: usize = 100_000;
/// The most points all the areas of one package may carry.
pub const MAX_OVERLAY_POINTS: usize = 5_000_000;
/// The most points one loop may have.
const MAX_RING_POINTS: usize = 20_000;
/// Segment pairs the loop checks may compare for the whole package.
const MAX_RING_WORK: u64 = 20_000_000;
/// The farthest from the origin an overlay point may be, in mm.
const MAX_COORDINATE: f64 = 1.0e6;
/// The most notes taken from a manifest; the rest are counted in one note.
const MAX_NOTES: usize = 1_000;
/// The longest note text kept, in characters.
const MAX_NOTE_CHARS: usize = 2_000;
/// The most left-out areas one note names.
const MAX_NAMED_AREAS: usize = 8;

/// Solder mask green, the default for copper under the mask.
const MASKED_COPPER: [f64; 3] = [0.05, 0.45, 0.22];
/// Gold, the default for bare copper.
const BARE_COPPER: [f64; 3] = [0.83, 0.64, 0.22];
/// White, the default for silkscreen.
const SILKSCREEN: [f64; 3] = [1.0, 1.0, 1.0];
/// Solder mask green, the default for a mask overlay.
const SOLDER_MASK: [f64; 3] = [0.05, 0.45, 0.22];

#[derive(Deserialize)]
struct Manifest {
    schema: String,
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    units: Option<String>,
    #[serde(default)]
    models: Vec<Value>,
    #[serde(default)]
    parts: Vec<Value>,
    #[serde(default)]
    overlays: Vec<Value>,
    #[serde(default)]
    notes: Vec<Value>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    file: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct PartEntry {
    name: String,
    model: String,
    #[serde(default)]
    transform: Option<[[f64; 4]; 3]>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    metallic: Option<f64>,
    #[serde(default)]
    roughness: Option<f64>,
    #[serde(default)]
    notes: Vec<Value>,
}

#[derive(Deserialize)]
struct OverlayEntry {
    name: String,
    kind: String,
    #[serde(default)]
    finish: Option<String>,
    #[serde(default)]
    part: Option<String>,
    #[serde(default)]
    side: Option<String>,
    #[serde(default)]
    z: Option<f64>,
    #[serde(default)]
    thickness: Option<f64>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    metallic: Option<f64>,
    #[serde(default)]
    roughness: Option<f64>,
    #[serde(default)]
    opacity: Option<f64>,
    #[serde(default)]
    areas: Vec<Value>,
}

#[derive(Deserialize)]
struct AreaEntry {
    outer: Vec<[f64; 2]>,
    #[serde(default)]
    holes: Vec<Vec<[f64; 2]>>,
}

/// One validated area: its loops without a repeated closing point.
#[derive(Debug)]
struct Area {
    outer: Vec<[f64; 2]>,
    holes: Vec<Vec<[f64; 2]>>,
}

/// How many millimetres one of the package's `units` is, when it names a known unit.
fn unit_scale(units: Option<&str>) -> Option<f64> {
    match units.map(str::trim) {
        None | Some("") | Some("mm") => Some(1.0),
        Some("cm") => Some(10.0),
        Some("m") => Some(1000.0),
        Some("in") => Some(25.4),
        Some(_) => None,
    }
}

/// Reads the package in `dir` whose manifest is `manifest` (the JSON a module prints) into a
/// model: an assembly of the package's parts, placed, with its overlays as sheet parts.
///
/// Errors refuse the whole package: a manifest that is too large, not JSON or not this schema,
/// a count over its cap, a file name outside the folder, or nothing left to place. A model whose
/// file is missing or unreadable, a part that cannot be placed and an overlay that cannot be
/// drawn are reported as losses and the rest converts.
pub fn read_package(dir: &Path, manifest: &[u8], report: &mut Report) -> Result<Model, Error> {
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err(refuse(format!(
            "the package manifest is {} bytes, over the {MAX_MANIFEST_BYTES}-byte limit",
            manifest.len()
        )));
    }
    let manifest: Manifest = serde_json::from_slice(manifest)
        .map_err(|err| refuse(format!("the package manifest is not valid: {err}")))?;
    if manifest.schema != PARTS_SCHEMA {
        return Err(refuse(format!(
            "the package manifest is `{}`, not `{PARTS_SCHEMA}`; update Dutchman or the module",
            clip(&manifest.schema, 64)
        )));
    }
    if manifest.ok == Some(false) {
        return Err(refuse(format!(
            "the module reported a failure{}",
            manifest
                .error
                .as_deref()
                .map(|e| format!(": {}", clip(e, MAX_NOTE_CHARS)))
                .unwrap_or_default()
        )));
    }
    let scale = unit_scale(manifest.units.as_deref()).ok_or_else(|| {
        refuse(format!(
            "the package uses the unit `{}`; Dutchman reads mm, cm, m and in",
            clip(manifest.units.as_deref().unwrap_or_default(), 32)
        ))
    })?;
    if manifest.models.len() > MAX_PARTS {
        return Err(refuse(format!(
            "the package lists {} models; Dutchman reads at most {MAX_PARTS}",
            manifest.models.len()
        )));
    }
    if manifest.parts.len() > MAX_PLACEMENTS {
        return Err(refuse(format!(
            "the package places {} parts; Dutchman reads at most {MAX_PLACEMENTS}",
            manifest.parts.len()
        )));
    }
    if manifest.overlays.len() > MAX_OVERLAYS {
        return Err(refuse(format!(
            "the package carries {} overlays; Dutchman reads at most {MAX_OVERLAYS}",
            manifest.overlays.len()
        )));
    }

    let mut notes = Notes::default();
    for note in &manifest.notes {
        notes.add(report, "package".to_owned(), note);
    }

    // Every model file is checked against the folder first, so one escaping name refuses the
    // package before any file is opened.
    let mut models: BTreeMap<String, (PathBuf, String, String)> = BTreeMap::new();
    for value in &manifest.models {
        let entry: ModelEntry = match serde_json::from_value(value.clone()) {
            Ok(entry) => entry,
            Err(err) => {
                report.push_at(
                    Stage::Input,
                    Severity::Degraded,
                    "package",
                    format!("a model entry is not valid and is skipped: {err}"),
                );
                continue;
            }
        };
        let path = inside(dir, &entry.file)
            .map_err(|why| refuse(format!("model \"{}\": {why}", clip(&entry.id, 64))))?;
        if models.contains_key(&entry.id) {
            report.push_at(
                Stage::Input,
                Severity::Degraded,
                "package",
                format!(
                    "model \"{}\" is listed twice; the first entry is used",
                    clip(&entry.id, 64)
                ),
            );
            continue;
        }
        let display = entry.name.unwrap_or_else(|| entry.id.clone());
        models.insert(entry.id, (path, entry.file, display));
    }

    let mut assembly = Assembly {
        own_part: None,
        parts: Vec::new(),
        definitions: Vec::new(),
        identities: Vec::new(),
        placements: Vec::new(),
        joints: Vec::new(),
        left_out: Vec::new(),
        held: Vec::new(),
        joint_notes: Vec::new(),
        stored_placements: Vec::new(),
    };
    let mut names = Names::default();
    let mut read: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let mut total_bytes = 0u64;
    // The part each placed name became, with its transform, for the overlays.
    let mut placed: BTreeMap<String, Placement3x4> = BTreeMap::new();
    let mut dropped_parts: BTreeSet<String> = BTreeSet::new();
    // The coloured copy of a model for each (model, colour) a part asks for.
    let mut variants: BTreeMap<(String, String, String), usize> = BTreeMap::new();

    for value in &manifest.parts {
        let entry: PartEntry = match serde_json::from_value(value.clone()) {
            Ok(entry) => entry,
            Err(err) => {
                left_out(
                    report,
                    &mut assembly,
                    None,
                    None,
                    format!("a part entry is not valid: {err}"),
                );
                continue;
            }
        };
        for note in &entry.notes {
            notes.add(report, format!("part \"{}\"", clip(&entry.name, 64)), note);
        }
        let Some((path, file, display)) = models.get(&entry.model) else {
            left_out(
                report,
                &mut assembly,
                Some(&entry.name),
                None,
                format!(
                    "part \"{}\" names model \"{}\", which the package does not list",
                    clip(&entry.name, 64),
                    clip(&entry.model, 64)
                ),
            );
            dropped_parts.insert(entry.name);
            continue;
        };
        let index = match read.get(&entry.model) {
            Some(index) => *index,
            None => {
                let index = read_model(
                    path,
                    file,
                    display,
                    &entry.model,
                    &mut total_bytes,
                    &mut assembly,
                    report,
                )?;
                read.insert(entry.model.clone(), index);
                index
            }
        };
        let Some(index) = index else {
            left_out(
                report,
                &mut assembly,
                Some(&entry.name),
                Some(file),
                format!(
                    "part \"{}\" is left out: its model file {} was not read",
                    clip(&entry.name, 64),
                    clip(file, 128)
                ),
            );
            dropped_parts.insert(entry.name);
            continue;
        };
        let mut transform = entry.transform.unwrap_or(IDENTITY);
        if transform.iter().flatten().any(|v| !v.is_finite()) {
            left_out(
                report,
                &mut assembly,
                Some(&entry.name),
                Some(file),
                format!(
                    "part \"{}\" is left out: its transform is not a finite 3 x 4 matrix",
                    clip(&entry.name, 64)
                ),
            );
            dropped_parts.insert(entry.name);
            continue;
        }
        if let Err(kind) = crate::placement::rigid(&transform) {
            left_out(
                report,
                &mut assembly,
                Some(&entry.name),
                Some(file),
                format!(
                    "part \"{}\" is left out: its transform is not a rigid rotation and shift ({kind})",
                    clip(&entry.name, 64)
                ),
            );
            dropped_parts.insert(entry.name);
            continue;
        }
        for row in &mut transform {
            if let Some(shift) = row.get_mut(3) {
                *shift *= scale;
            }
        }
        if transform.iter().flatten().any(|v| !v.is_finite()) {
            left_out(
                report,
                &mut assembly,
                Some(&entry.name),
                Some(file),
                format!(
                    "part \"{}\" is left out: its shift is not finite once scaled to mm",
                    clip(&entry.name, 64)
                ),
            );
            dropped_parts.insert(entry.name);
            continue;
        }
        let mut look = match surface(entry.metallic, entry.roughness) {
            Ok(look) => look,
            Err(why) => {
                left_out(
                    report,
                    &mut assembly,
                    Some(&entry.name),
                    Some(file),
                    format!("part \"{}\" is left out: {why}", clip(&entry.name, 64)),
                );
                dropped_parts.insert(entry.name);
                continue;
            }
        };
        if look.is_some() && entry.color.is_none() {
            report.push_at(
                Stage::Input,
                Severity::Degraded,
                format!("part \"{}\"", clip(&entry.name, 64)),
                "its metallic and roughness are not applied: they go with a `color`".to_owned(),
            );
            look = None;
        }
        let index = match entry.color.as_deref() {
            None => index,
            Some(text) => {
                let colored = parse_color(text)
                    .ok_or_else(|| format!("its color `{}` is not #rrggbb", clip(text, 16)));
                let key = (
                    entry.model.clone(),
                    text.to_ascii_lowercase(),
                    surface_key(look),
                );
                let made = colored.and_then(|rgb| match variants.get(&key) {
                    Some(at) => Ok(*at),
                    None => {
                        let at = colour_variant(
                            &mut assembly,
                            index,
                            rgb,
                            text,
                            look,
                            &entry.model,
                            report,
                        )?;
                        variants.insert(key, at);
                        Ok(at)
                    }
                });
                match made {
                    Ok(at) => at,
                    Err(why) => {
                        left_out(
                            report,
                            &mut assembly,
                            Some(&entry.name),
                            Some(file),
                            format!("part \"{}\" is left out: {why}", clip(&entry.name, 64)),
                        );
                        dropped_parts.insert(entry.name);
                        continue;
                    }
                }
            }
        };
        let name = names.unique(&entry.name);
        if name != entry.name {
            report.push_at(
                Stage::Input,
                Severity::Degraded,
                format!("part \"{}\"", clip(&entry.name, 64)),
                format!(
                    "the package names two parts \"{}\"; the second is placed as \"{name}\"",
                    clip(&entry.name, 64)
                ),
            );
        }
        // The plating is one tube per plated hole: SolidCraft holds them as one body.
        if entry.name == "Plating" {
            if let Some(part) = assembly.parts.get_mut(index) {
                part.merge_bodies = true;
            }
        }
        // The first part of a name is the one an overlay on that name follows.
        placed.entry(entry.name.clone()).or_insert(transform);
        assembly.placements.push(Placement {
            unproven: false,
            name: Some(name),
            part: index,
            transform,
            hidden: false,
        });
    }

    overlays(
        &manifest.overlays,
        scale,
        &placed,
        &dropped_parts,
        &mut names,
        &mut assembly,
        report,
    );

    drop_unplaced_parts(&mut assembly);
    if assembly.placements.is_empty() {
        return Err(refuse(
            "the package places no parts Dutchman could read".to_owned(),
        ));
    }
    notes.finish(report);
    report.set_completeness(assembly.completeness());
    let name = manifest
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map_or_else(|| "Parts package".to_owned(), |name| clip(name, 200));
    report.saw(&name);
    Ok(Model {
        ir: cadmpeg_ir::CadIr::empty(),
        fidelity: None,
        name,
        source_file: None,
        assembly: Some(assembly),
        source_step: None,
        merge_bodies: false,
    })
}

/// Removes the parts no placement uses, such as a model every placement painted with its own
/// colour: written as a component with no occurrence, SolidCraft 0.46 and 0.47 still draw its
/// uncoloured body over the coloured copy's.
fn drop_unplaced_parts(assembly: &mut Assembly) {
    let used: BTreeSet<usize> = assembly.placements.iter().map(|p| p.part).collect();
    if used.len() == assembly.parts.len() {
        return;
    }
    let mut next = BTreeMap::new();
    let (mut parts, mut definitions, mut identities) = (Vec::new(), Vec::new(), Vec::new());
    for (old, part) in std::mem::take(&mut assembly.parts).into_iter().enumerate() {
        if !used.contains(&old) {
            continue;
        }
        next.insert(old, parts.len());
        parts.push(part);
        definitions.push(assembly.definitions.get(old).cloned().flatten());
        identities.push(assembly.identities.get(old).cloned().flatten());
    }
    for placement in &mut assembly.placements {
        placement.part = next.get(&placement.part).copied().unwrap_or(placement.part);
    }
    assembly.parts = parts;
    assembly.definitions = definitions;
    assembly.identities = identities;
}

/// A body's `metallic` and `roughness`, each 0 to 1, as the pair SolidCraft's look takes. A
/// value left out is 0 for `metallic` and 0.5 for `roughness`; neither given is no pair. A value
/// outside 0 to 1 is an error that names it.
fn surface(metallic: Option<f64>, roughness: Option<f64>) -> Result<Option<[f64; 2]>, String> {
    for (what, value) in [("metallic", metallic), ("roughness", roughness)] {
        if let Some(v) = value {
            if !(v.is_finite() && (0.0..=1.0).contains(&v)) {
                return Err(format!("its {what} {v} is not a number from 0 to 1"));
            }
        }
    }
    Ok(match (metallic, roughness) {
        (None, None) => None,
        (m, r) => Some([m.unwrap_or(0.0), r.unwrap_or(0.5)]),
    })
}

/// A copy of part `index` whose bodies carry the colour `text`, as appearance bindings, the way
/// every reader gives a body its colour. A model that is a sheet (no solid bodies) keeps its
/// own look, with a note.
fn colour_variant(
    assembly: &mut Assembly,
    index: usize,
    rgb: [f64; 3],
    text: &str,
    surface: Option<[f64; 2]>,
    model: &str,
    report: &mut Report,
) -> Result<usize, String> {
    let Some(source) = assembly.parts.get(index) else {
        return Err("its model was not read".to_owned());
    };
    if source.ir.geometry.bodies.is_empty() {
        report.push_at(
            Stage::Input,
            Severity::Degraded,
            format!("model \"{}\"", clip(model, 64)),
            format!(
                "its color `{}` is not applied: the model has no solid body to color",
                clip(text, 16)
            ),
        );
        return Ok(index);
    }
    if assembly.parts.len() >= MAX_PARTS {
        return Err(format!(
            "the package would pass {MAX_PARTS} parts with its colors"
        ));
    }
    let mut copy = source.clone();
    // The writer takes a body's look from its appearance bindings when it writes the part from
    // its geometry, not from the STEP text a part was read from (the way a mirrored part is
    // restated).
    copy.source_step = None;
    copy.fidelity = None;
    let hex = text.trim_start_matches('#').to_ascii_lowercase();
    bind_body_colour(&mut copy, rgb, 1.0, surface, &hex)?;
    let at = assembly.parts.len();
    assembly.parts.push(copy);
    assembly.definitions.push(None);
    assembly.identities.push(Some(format!(
        "{PARTS_SCHEMA}:{model}:{hex}{}",
        surface_key(surface)
    )));
    Ok(at)
}

/// Gives every body of `part` the colour `rgb` with opacity `alpha` as one appearance and a
/// binding per body, replacing its other body and face bindings, so the writer records the look by body.
/// The faces' and bodies' own colours are cleared, so the binding alone gives the look. `key`
/// names the appearance and its bindings.
fn bind_body_colour(
    part: &mut Model,
    rgb: [f64; 3],
    alpha: f64,
    surface: Option<[f64; 2]>,
    key: &str,
) -> Result<(), String> {
    let key = if alpha < 1.0 {
        format!("{key}-a{}", (alpha * 1000.0).round())
    } else {
        key.to_owned()
    };
    let key = format!("{key}{}", surface_key(surface));
    let appearance = format!("dutchman:parts:appearance#{key}");
    part.ir.model.appearance_bindings.retain(|binding| {
        !matches!(
            binding.target.entity,
            cadmpeg_ir::appearance::AppearanceTarget::Body(_)
                | cadmpeg_ir::appearance::AppearanceTarget::Face(_)
        )
    });
    if !part
        .ir
        .model
        .appearances
        .iter()
        .any(|a| a.id.as_str() == appearance)
    {
        let properties = match surface {
            Some([metallic, roughness]) => json!({"metallic": metallic, "roughness": roughness}),
            None => json!({}),
        };
        let made = serde_json::from_value(json!({
            "id": appearance,
            "base_color": {"r": rgb[0], "g": rgb[1], "b": rgb[2], "a": alpha},
            "properties": properties,
            "textures": [],
        }));
        match made {
            Ok(made) => part.ir.model.appearances.push(made),
            Err(err) => return Err(format!("its color cannot be recorded: {err}")),
        }
    }
    let ids: Vec<Value> = part
        .ir
        .geometry
        .bodies
        .iter()
        .filter_map(|body| serde_json::to_value(&body.id).ok())
        .collect();
    for (n, id) in ids.into_iter().enumerate() {
        let binding = serde_json::from_value(json!({
            "id": format!("dutchman:parts:binding#{key}-{n}"),
            "target": {"entity": {"kind": "body", "value": id}},
            "appearance": appearance,
            "channels": {},
        }));
        match binding {
            Ok(binding) => part.ir.model.appearance_bindings.push(binding),
            Err(err) => return Err(format!("its color cannot be recorded: {err}")),
        }
    }
    // The binding is the one source of the look. A colour left on a face would be written as a
    // face look of its own beside the body's, which paints those faces unlike the rest.
    for face in &mut part.ir.geometry.faces {
        face.color = None;
    }
    for body in &mut part.ir.geometry.bodies {
        body.color = None;
    }
    Ok(())
}

/// The part of an appearance's name that tells apart its `metallic` and `roughness`.
fn surface_key(surface: Option<[f64; 2]>) -> String {
    surface.map_or_else(String::new, |[m, r]| {
        format!("-m{}r{}", (m * 1000.0).round(), (r * 1000.0).round())
    })
}

/// One colour component in 0 to 1 as two hex digits.
fn rgb_hex(c: f64) -> String {
    format!("{:02x}", (c.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn refuse(message: String) -> Error {
    Error::Assembly(format!("the parts package is refused: {message}"))
}

/// The first `max` characters of `text`.
fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// `file` as a path inside `dir`: relative, no `..`, not through a link that leaves the folder.
fn inside(dir: &Path, file: &str) -> Result<PathBuf, String> {
    let shown = clip(file, 128);
    if file.is_empty() || file.contains(['\\', '\0']) {
        return Err(format!(
            "the file name `{shown}` is not a relative path with `/` separators"
        ));
    }
    let relative = Path::new(file);
    if relative.components().any(|part| {
        matches!(
            part,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!("the file `{shown}` is outside the package folder"));
    }
    let path = dir.join(relative);
    if let (Ok(real), Ok(base)) = (path.canonicalize(), dir.canonicalize()) {
        if !real.starts_with(&base) {
            return Err(format!("the file `{shown}` is outside the package folder"));
        }
    }
    Ok(path)
}

/// Reads one model file through the decode path and adds it as a part. `Ok(None)` after a
/// loss: the file is missing, too large or does not decode.
fn read_model(
    path: &Path,
    file: &str,
    display: &str,
    id: &str,
    total_bytes: &mut u64,
    assembly: &mut Assembly,
    report: &mut Report,
) -> Result<Option<usize>, Error> {
    let subject = format!("model \"{}\"", clip(id, 64));
    let drop_note = |report: &mut Report, why: String| {
        report.push_at(Stage::Input, Severity::Dropped, subject.clone(), why);
    };
    let meta = match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => meta,
        Ok(_) => {
            drop_note(
                report,
                format!("its file {} is not a file", clip(file, 128)),
            );
            return Ok(None);
        }
        Err(_) => {
            drop_note(
                report,
                format!("its file {} is missing from the package", clip(file, 128)),
            );
            return Ok(None);
        }
    };
    if meta.len() > MAX_INPUT_BYTES {
        drop_note(
            report,
            format!(
                "its file {} is {} bytes, over the {MAX_INPUT_BYTES}-byte limit",
                clip(file, 128),
                meta.len()
            ),
        );
        return Ok(None);
    }
    *total_bytes = total_bytes.saturating_add(meta.len());
    if *total_bytes > MAX_PACKAGE_BYTES {
        return Err(refuse(format!(
            "the model files add up to more than {MAX_PACKAGE_BYTES} bytes"
        )));
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            drop_note(
                report,
                format!("its file {} cannot be read: {err}", clip(file, 128)),
            );
            return Ok(None);
        }
    };
    let mut scratch = Report::default();
    let name = Path::new(file).file_name().map_or_else(
        || PathBuf::from("model.step"),
        |name| PathBuf::from(name.to_owned()),
    );
    let model = crate::read_bytes(Format::Step, &name, &bytes, &mut scratch);
    // The package says where each part goes: a model read on its own is not placed by a guess.
    scratch.forget_part_positions();
    report.append_prefixed(scratch, &subject);
    match model {
        Ok(mut model) => {
            model.name = clip(display, 200);
            model.source_file = None;
            let index = assembly.parts.len();
            assembly.parts.push(model);
            assembly.definitions.push(None);
            // Parts placed from one model are one definition.
            assembly
                .identities
                .push(Some(format!("{PARTS_SCHEMA}:{id}")));
            Ok(Some(index))
        }
        Err(err) => {
            drop_note(
                report,
                format!("its file {} did not convert: {err}", clip(file, 128)),
            );
            Ok(None)
        }
    }
}

/// Records a part that is not placed, for the report and the completeness count.
fn left_out(
    report: &mut Report,
    assembly: &mut Assembly,
    name: Option<&str>,
    file: Option<&str>,
    reason: String,
) {
    let subject = name.map_or_else(
        || "part".to_owned(),
        |name| format!("part \"{}\"", clip(name, 64)),
    );
    report.push_at(Stage::Input, Severity::Dropped, subject, reason.clone());
    assembly.left_out.push(LeftOut {
        name: name.map(|name| clip(name, 64)),
        file: file.map(|file| clip(file, 128)),
        reason,
        stage: Stage::Input,
    });
}

/// Hands out each placed name once: the second use of a name gets ` (2)`, and so on.
#[derive(Default)]
struct Names {
    used: BTreeSet<String>,
}

impl Names {
    fn unique(&mut self, wanted: &str) -> String {
        let wanted = clip(wanted, 200);
        let mut name = wanted.clone();
        let mut n = 2usize;
        while !self.used.insert(name.clone()) {
            name = format!("{wanted} ({n})");
            n = n.saturating_add(1);
        }
        name
    }
}

/// The notes taken from the manifest, capped.
#[derive(Default)]
struct Notes {
    taken: usize,
    skipped: usize,
}

impl Notes {
    fn add(&mut self, report: &mut Report, subject: String, note: &Value) {
        if self.taken >= MAX_NOTES {
            self.skipped = self.skipped.saturating_add(1);
            return;
        }
        self.taken = self.taken.saturating_add(1);
        let message = note
            .get("message")
            .and_then(Value::as_str)
            .map(|m| clip(m, MAX_NOTE_CHARS));
        let Some(message) = message else {
            return;
        };
        let (severity, message) = match note.get("severity").and_then(Value::as_str) {
            Some("Info") => (Severity::Info, message),
            Some("Degraded") => (Severity::Degraded, message),
            Some("Dropped") => (Severity::Dropped, message),
            // A severity this reader does not know is shown, never dropped from view.
            other => (
                Severity::Degraded,
                format!("{message} (severity {})", other.unwrap_or("missing")),
            ),
        };
        report.push_at(Stage::Input, severity, subject, message);
    }

    fn finish(&self, report: &mut Report) {
        if self.skipped > 0 {
            report.push_at(
                Stage::Input,
                Severity::Info,
                "package",
                format!(
                    "{} more notes in the package were not read (the limit is {MAX_NOTES})",
                    self.skipped
                ),
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Overlays
// ---------------------------------------------------------------------------------------------

/// Adds each overlay as a sheet part placed with the part it names.
fn overlays(
    list: &[Value],
    scale: f64,
    placed: &BTreeMap<String, Placement3x4>,
    dropped_parts: &BTreeSet<String>,
    names: &mut Names,
    assembly: &mut Assembly,
    report: &mut Report,
) {
    let mut points_left = MAX_OVERLAY_POINTS;
    let mut work_left = MAX_RING_WORK;
    let mut faces_left = MAX_SOLID_FACES;
    for value in list {
        let entry: OverlayEntry = match serde_json::from_value(value.clone()) {
            Ok(entry) => entry,
            Err(err) => {
                report.push_at(
                    Stage::Input,
                    Severity::Dropped,
                    "overlay",
                    format!("an overlay entry is not valid and is left out: {err}"),
                );
                continue;
            }
        };
        let subject = format!("overlay \"{}\"", clip(&entry.name, 64));
        let dropped = |report: &mut Report, why: String| {
            report.push_at(Stage::Input, Severity::Dropped, subject.clone(), why);
        };
        let color = match entry.kind.as_str() {
            "copper" => match entry.finish.as_deref() {
                Some("bare") => BARE_COPPER,
                _ => MASKED_COPPER,
            },
            "silkscreen" => SILKSCREEN,
            "mask" => SOLDER_MASK,
            other => {
                dropped(
                    report,
                    format!(
                        "its kind `{}` is not copper, silkscreen or mask; it is left out",
                        clip(other, 32)
                    ),
                );
                continue;
            }
        };
        let color = match entry.color.as_deref() {
            None => color,
            Some(text) => {
                let Some(parsed) = parse_color(text) else {
                    dropped(
                        report,
                        format!(
                            "its color `{}` is not #rrggbb; it is left out",
                            clip(text, 16)
                        ),
                    );
                    continue;
                };
                parsed
            }
        };
        let transparency = match (entry.kind.as_str(), entry.opacity) {
            (_, None) => None,
            ("mask", Some(o)) if o.is_finite() && (0.0..=1.0).contains(&o) => Some(1.0 - o),
            ("mask", Some(o)) => {
                dropped(
                    report,
                    format!("its opacity {o} is not a number from 0 to 1; it is left out"),
                );
                continue;
            }
            (_, Some(_)) => {
                report.push_at(
                    Stage::Input,
                    Severity::Degraded,
                    subject.clone(),
                    "its opacity is read on a mask only; it is ignored here".to_owned(),
                );
                None
            }
        };
        let look = match surface(entry.metallic, entry.roughness) {
            Ok(look) => look,
            Err(why) => {
                dropped(report, format!("{why}; it is left out"));
                continue;
            }
        };
        let look = if look.is_some() && transparency.is_some_and(|t| t > 0.0) {
            report.push_at(
                Stage::Input,
                Severity::Degraded,
                subject.clone(),
                "its metallic and roughness are not applied: SolidCraft takes them on an opaque look only".to_owned(),
            );
            None
        } else {
            look
        };
        let Some(part_name) = entry.part.as_deref() else {
            dropped(
                report,
                "it names no part to be drawn on; it is left out".to_owned(),
            );
            continue;
        };
        let Some(transform) = placed.get(part_name) else {
            let why = if dropped_parts.contains(part_name) {
                "is not placed"
            } else {
                "is not in the package"
            };
            dropped(
                report,
                format!(
                    "the part \"{}\" it is drawn on {why}; it is left out",
                    clip(part_name, 64)
                ),
            );
            continue;
        };
        let Some(z) = entry
            .z
            .filter(|z| z.is_finite() && z.abs() <= MAX_COORDINATE)
        else {
            dropped(
                report,
                "it has no finite height `z`; it is left out".to_owned(),
            );
            continue;
        };
        let thickness = match entry.thickness {
            None => None,
            Some(0.0) => None,
            Some(t) if t.is_finite() && t > 0.0 && t <= 1.0 => Some(t),
            Some(t) => {
                dropped(
                    report,
                    format!("its thickness {t} is not a number above 0 up to 1 mm; it is left out"),
                );
                continue;
            }
        };
        if entry.areas.len() > MAX_AREAS {
            dropped(
                report,
                format!(
                    "it has {} areas; Dutchman draws at most {MAX_AREAS} per overlay",
                    entry.areas.len()
                ),
            );
            continue;
        }
        let mut areas = Vec::new();
        let mut bad: Vec<String> = Vec::new();
        let mut bad_count = 0usize;
        for (number, area) in entry.areas.iter().enumerate() {
            let checked = serde_json::from_value::<AreaEntry>(area.clone())
                .map_err(|err| format!("is not valid: {err}"))
                .and_then(|area| {
                    let used = area
                        .outer
                        .len()
                        .saturating_add(area.holes.iter().map(Vec::len).sum::<usize>());
                    if used > points_left {
                        return Err("would pass the point limit for the package".to_owned());
                    }
                    points_left -= used;
                    validate_area(area, scale, &mut work_left)
                });
            match checked {
                Ok(area) => areas.push(area),
                Err(why) => {
                    bad_count = bad_count.saturating_add(1);
                    if bad.len() < MAX_NAMED_AREAS {
                        bad.push(format!("area {}: {why}", number.saturating_add(1)));
                    }
                }
            }
        }
        if bad_count > 0 {
            report.push_at(
                Stage::Input,
                Severity::Degraded,
                subject.clone(),
                format!(
                    "{bad_count} area(s) left out ({}{})",
                    bad.join("; "),
                    if bad_count > bad.len() { "; ..." } else { "" }
                ),
            );
        }
        if areas.is_empty() {
            dropped(
                report,
                "none of its areas could be drawn; it is left out".to_owned(),
            );
            continue;
        }
        let z = z * scale;
        let cost = areas
            .iter()
            .map(prism_faces)
            .fold(0usize, usize::saturating_add);
        let solid = match thickness {
            Some(t) if cost <= faces_left => {
                faces_left -= cost;
                Some(t)
            }
            Some(_) => {
                report.push_at(
                    Stage::Input,
                    Severity::Degraded,
                    subject.clone(),
                    format!(
                        "its thickness would pass the package's limit of {MAX_SOLID_FACES} solid \
                         faces ({cost} more needed, {faces_left} left); it is drawn as a sheet"
                    ),
                );
                None
            }
            None => None,
        };
        let text = match solid {
            Some(t) if entry.side.as_deref() == Some("bottom") => {
                solid_step(&entry.name, z - t, z, &areas, color, transparency)
            }
            Some(t) => solid_step(&entry.name, z, z + t, &areas, color, transparency),
            None => sheet_step(&entry.name, z, &areas, color, transparency),
        };
        let mut scratch = Report::default();
        let model = crate::read_bytes(
            Format::Step,
            Path::new("overlay.step"),
            text.as_bytes(),
            &mut scratch,
        );
        scratch.forget_part_positions();
        report.append_prefixed(scratch, &subject);
        match model {
            Ok(mut model) => {
                model.name = clip(&entry.name, 200);
                model.source_file = None;
                // One closed solid per area is one overlay to the person; sheets stay apart.
                model.merge_bodies = solid.is_some();
                // The look goes in as body bindings, which the writer reads when it restates the part.
                let opacity = transparency.map_or(1.0, |t| 1.0 - t);
                let hex = format!(
                    "{}{}{}",
                    rgb_hex(color[0]),
                    rgb_hex(color[1]),
                    rgb_hex(color[2])
                );
                if let Err(why) = bind_body_colour(&mut model, color, opacity, look, &hex) {
                    report.push_at(Stage::Input, Severity::Degraded, subject.clone(), why);
                }
                let index = assembly.parts.len();
                if index >= MAX_PARTS {
                    dropped(
                        report,
                        format!("the package would pass {MAX_PARTS} parts; it is left out"),
                    );
                    continue;
                }
                assembly.parts.push(model);
                assembly.definitions.push(None);
                assembly.identities.push(None);
                let name = names.unique(&entry.name);
                if name != entry.name {
                    report.push_at(
                        Stage::Input,
                        Severity::Degraded,
                        subject.clone(),
                        format!("its name is already a part's; it is placed as \"{name}\""),
                    );
                }
                assembly.placements.push(Placement {
                    unproven: false,
                    name: Some(name),
                    part: index,
                    transform: *transform,
                    hidden: false,
                });
            }
            Err(err) => dropped(report, format!("could not be drawn: {err}")),
        }
    }
}

/// `#rrggbb` as three components in 0 to 1.
fn parse_color(text: &str) -> Option<[f64; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |at: usize| {
        let pair = hex.get(at..at + 2)?;
        Some(f64::from(u8::from_str_radix(pair, 16).ok()?) / 255.0)
    };
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// Twice the signed area of a loop (positive when counter-clockwise).
fn twice_area(ring: &[[f64; 2]]) -> f64 {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum()
}

/// Whether two points are closer than the shortest edge that can be written (1e-6 mm).
fn near(a: &[f64; 2], b: &[f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6
}

/// A loop as drawn: finite, in range, without repeated points or a repeated closing point, with
/// at least three points, scaled to mm.
fn clean_ring(points: &[[f64; 2]], scale: f64, what: &str) -> Result<Vec<[f64; 2]>, String> {
    if points.len() < 3 {
        return Err(format!("{what} has fewer than 3 points"));
    }
    if points.len() > MAX_RING_POINTS {
        return Err(format!(
            "{what} has {} points; the limit is {MAX_RING_POINTS}",
            points.len()
        ));
    }
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    for p in points {
        let q = [p[0] * scale, p[1] * scale];
        if !(q[0].is_finite()
            && q[1].is_finite()
            && q[0].abs() <= MAX_COORDINATE
            && q[1].abs() <= MAX_COORDINATE)
        {
            return Err(format!(
                "{what} has a point that is not finite or too far out"
            ));
        }
        if out.last().is_none_or(|last| !near(last, &q)) {
            out.push(q);
        }
    }
    while out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| near(a, b)) {
        out.pop();
    }
    if out.len() < 3 {
        return Err(format!("{what} has fewer than 3 different points"));
    }
    Ok(out)
}

/// Checks one area: finite points, at least 3 in every loop, the outer loop counter-clockwise,
/// holes clockwise and inside it (and not inside each other), no loop crossing itself, and no
/// two loops crossing or sharing part of an edge. Loops may touch at a point.
fn validate_area(area: AreaEntry, scale: f64, work_left: &mut u64) -> Result<Area, String> {
    let outer = clean_ring(&area.outer, scale, "the outer loop")?;
    let outer_area = twice_area(&outer);
    if outer_area == 0.0 {
        return Err("the outer loop has no area".to_owned());
    }
    if outer_area < 0.0 {
        return Err("the outer loop runs clockwise, not counter-clockwise".to_owned());
    }
    let mut holes: Vec<Vec<[f64; 2]>> = Vec::with_capacity(area.holes.len());
    for (n, hole) in area.holes.iter().enumerate() {
        let what = format!("hole {}", n.saturating_add(1));
        let ring = clean_ring(hole, scale, &what)?;
        let hole_area = twice_area(&ring);
        if hole_area == 0.0 {
            return Err(format!("{what} has no area"));
        }
        if hole_area > 0.0 {
            return Err(format!("{what} runs counter-clockwise, not clockwise"));
        }
        if !within(&ring, &outer, work_left)? {
            return Err(format!("{what} is not inside the outer loop"));
        }
        for (other, earlier) in holes.iter().enumerate() {
            if within(&ring, earlier, work_left)? || within(earlier, &ring, work_left)? {
                return Err(format!(
                    "{what} is inside hole {}; a hole inside a hole is not drawn",
                    other.saturating_add(1)
                ));
            }
        }
        holes.push(ring);
    }
    let mut rings: Vec<&[[f64; 2]]> = vec![outer.as_slice()];
    rings.extend(holes.iter().map(Vec::as_slice));
    if let Some(what) = crossing(&rings, work_left) {
        return Err(what);
    }
    Ok(Area { outer, holes })
}

/// Whether `inner` lies inside `outer`: judged by the first of its first few points that is not
/// on `outer`'s boundary, so a loop touching `outer` at a point still counts as inside. Charges
/// the work limit.
fn within(inner: &[[f64; 2]], outer: &[[f64; 2]], work_left: &mut u64) -> Result<bool, String> {
    for p in inner.iter().take(8) {
        let cost = u64::try_from(outer.len()).unwrap_or(u64::MAX);
        if *work_left < cost {
            *work_left = 0;
            return Err(WORK_LIMIT.to_owned());
        }
        *work_left = work_left.saturating_sub(cost);
        let on_edge = outer
            .iter()
            .zip(outer.iter().cycle().skip(1))
            .any(|(a, b)| point_segment(*p, *a, *b) < TOUCH);
        if !on_edge {
            return Ok(inside_ring(outer, *p));
        }
    }
    Ok(false)
}

const WORK_LIMIT: &str = "the package has more loop checking to do than the work limit allows";

/// Whether `p` is inside `ring` (even-odd).
fn inside_ring(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Two points closer than this (mm) are one point.
const TOUCH: f64 = 1e-6;

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The distance from `p` to the segment `a b`.
fn point_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    if len2 == 0.0 {
        return dist(p, a);
    }
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0);
    dist(p, [a[0] + t * d[0], a[1] + t * d[1]])
}

/// How two segments meet.
#[derive(PartialEq)]
enum Contact {
    Apart,
    /// They meet at a point (a shared vertex, or a vertex on the other's edge).
    Touch,
    /// They cross each other.
    Cross,
    /// They share part of an edge.
    Overlap,
}

fn contact(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Contact {
    let ends = [(a, b), (b, a)];
    let others = [(c, d), (d, c)];
    let mut shared = Vec::new();
    for (p, x) in ends {
        for (q, y) in others {
            if dist(p, q) < TOUCH {
                shared.push((p, x, y));
            }
        }
    }
    if shared.len() >= 2 {
        // Both ends meet: the same edge, or a pair of edges with a repeated vertex.
        return Contact::Overlap;
    }
    if let Some((p, x, y)) = shared.first() {
        // One shared vertex: they only overlap when they leave it the same way.
        let u = [x[0] - p[0], x[1] - p[1]];
        let v = [y[0] - p[0], y[1] - p[1]];
        let cross = u[0] * v[1] - u[1] * v[0];
        let dot = u[0] * v[0] + u[1] * v[1];
        let longest = u[0].hypot(u[1]).max(v[0].hypot(v[1]));
        return if dot > 0.0 && cross.abs() <= TOUCH * longest {
            Contact::Overlap
        } else {
            Contact::Touch
        };
    }
    let (d1, d2) = (orient(c, d, a), orient(c, d, b));
    let (d3, d4) = (orient(a, b, c), orient(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return Contact::Cross;
    }
    let length = dist(a, b);
    if length > 0.0 && point_line(c, a, b, length) <= TOUCH && point_line(d, a, b, length) <= TOUCH
    {
        // On one line: how far they overlap along it.
        let along =
            |p: [f64; 2]| ((p[0] - a[0]) * (b[0] - a[0]) + (p[1] - a[1]) * (b[1] - a[1])) / length;
        let (lo, hi) = (along(c).min(along(d)), along(c).max(along(d)));
        let overlap = hi.min(length) - lo.max(0.0);
        return if overlap > TOUCH {
            Contact::Overlap
        } else if overlap >= -TOUCH {
            Contact::Touch
        } else {
            Contact::Apart
        };
    }
    if point_segment(a, c, d) < TOUCH
        || point_segment(b, c, d) < TOUCH
        || point_segment(c, a, b) < TOUCH
        || point_segment(d, a, b) < TOUCH
    {
        return Contact::Touch;
    }
    Contact::Apart
}

/// The distance from `p` to the line through `a b` (of length `length`).
fn point_line(p: [f64; 2], a: [f64; 2], b: [f64; 2], length: f64) -> f64 {
    orient(a, b, p).abs() / length
}

/// A segment of one loop, for the crossing sweep.
struct Segment {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    ring: usize,
    index: usize,
    len: usize,
    a: [f64; 2],
    b: [f64; 2],
}

/// Why two segments of `rings` cross or overlap where they should not, found by sweeping the
/// segments in x order. A loop may touch another at a point, never itself. Every pair the sweep
/// compares is charged to `work_left`.
fn crossing(rings: &[&[[f64; 2]]], work_left: &mut u64) -> Option<String> {
    let mut segments = Vec::new();
    for (ring_index, ring) in rings.iter().enumerate() {
        for (index, (a, b)) in ring.iter().zip(ring.iter().cycle().skip(1)).enumerate() {
            segments.push(Segment {
                min_x: a[0].min(b[0]),
                max_x: a[0].max(b[0]),
                min_y: a[1].min(b[1]),
                max_y: a[1].max(b[1]),
                ring: ring_index,
                index,
                len: ring.len(),
                a: *a,
                b: *b,
            });
        }
    }
    segments.sort_by(|x, y| x.min_x.total_cmp(&y.min_x));
    for (i, s) in segments.iter().enumerate() {
        for t in segments.iter().skip(i.saturating_add(1)) {
            if t.min_x > s.max_x + TOUCH {
                break;
            }
            if *work_left == 0 {
                return Some(WORK_LIMIT.to_owned());
            }
            *work_left = work_left.saturating_sub(1);
            if t.min_y > s.max_y + TOUCH || s.min_y > t.max_y + TOUCH {
                continue;
            }
            let same = s.ring == t.ring;
            let next_to = same && {
                let apart = s.index.abs_diff(t.index);
                apart == 1 || apart == s.len.saturating_sub(1)
            };
            match contact(s.a, s.b, t.a, t.b) {
                Contact::Apart => {}
                Contact::Touch if next_to => {}
                Contact::Touch if !same => {}
                Contact::Overlap if !same => {
                    return Some("two loops share part of an edge".to_owned());
                }
                Contact::Cross if !same => return Some("two loops cross or touch".to_owned()),
                _ => return Some("a loop crosses or touches itself".to_owned()),
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// STEP text
// ---------------------------------------------------------------------------------------------

/// A STEP real: always with a decimal point, never `-0.`.
fn real(v: f64) -> String {
    let v = if v.abs() < 1e-12 { 0.0 } else { v };
    let mut s = format!("{v:.9}");
    while s.ends_with('0') {
        s.pop();
    }
    s
}

/// A STEP string: printable ASCII kept, quotes and backslashes doubled, other characters in
/// the `\X2\` and `\X4\` forms.
fn text(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            c if c.is_control() => out.push('_'),
            c if u32::from(c) <= 0xFFFF => {
                let _ = write!(out, "\\X2\\{:04X}\\X0\\", u32::from(c));
            }
            c => {
                let _ = write!(out, "\\X4\\{:08X}\\X0\\", u32::from(c));
            }
        }
    }
    out.push('\'');
    out
}

/// Collects numbered entities.
#[derive(Default)]
struct Step {
    lines: Vec<String>,
}

impl Step {
    fn add(&mut self, entity: String) -> usize {
        self.lines.push(entity);
        self.lines.len()
    }

    fn point(&mut self, p: [f64; 3]) -> usize {
        self.add(format!(
            "CARTESIAN_POINT('',({},{},{}))",
            real(p[0]),
            real(p[1]),
            real(p[2])
        ))
    }

    fn direction(&mut self, d: [f64; 3]) -> usize {
        self.add(format!(
            "DIRECTION('',({},{},{}))",
            real(d[0]),
            real(d[1]),
            real(d[2])
        ))
    }

    fn placement(&mut self, origin: [f64; 3], axis: [f64; 3], reference: [f64; 3]) -> usize {
        let p = self.point(origin);
        let a = self.direction(axis);
        let r = self.direction(reference);
        self.add(format!("AXIS2_PLACEMENT_3D('',#{p},#{a},#{r})"))
    }

    /// An infinite line through `from` towards `to`, and the vertices at both ends.
    fn edge(&mut self, from: usize, to: usize, a: [f64; 3], b: [f64; 3]) -> usize {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let p = self.point(a);
        let dir = self.direction([d[0] / len, d[1] / len, d[2] / len]);
        let v = self.add(format!("VECTOR('',#{dir},{})", real(len)));
        let line = self.add(format!("LINE('',#{p},#{v})"));
        self.add(format!("EDGE_CURVE('',#{from},#{to},#{line},.T.)"))
    }

    fn vertex(&mut self, p: [f64; 3]) -> usize {
        let point = self.point(p);
        self.add(format!("VERTEX_POINT('',#{point})"))
    }

    fn edge_loop(&mut self, edges: &[(usize, bool)]) -> usize {
        let ids: Vec<String> = edges
            .iter()
            .map(|(edge, forward)| {
                let sense = if *forward { ".T." } else { ".F." };
                format!(
                    "#{}",
                    self.add(format!("ORIENTED_EDGE('',*,*,#{edge},{sense})"))
                )
            })
            .collect();
        self.add(format!("EDGE_LOOP('',({}))", ids.join(",")))
    }

    /// The file: `shape` is the entity name of the shape representation, `bodies` the entities
    /// that hold the geometry, `styled` the styled items to put in a presentation of it.
    fn finish(mut self, name: &str, shape: &str, bodies: &[usize], styled: &[usize]) -> String {
        let origin = self.placement([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let length = self.add("( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) )".into());
        let angle = self.add("( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )".into());
        let solid_angle =
            self.add("( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() )".into());
        let uncertainty = self.add(format!(
            "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{length},\
             'distance_accuracy_value','confusion accuracy')"
        ));
        let context = self.add(format!(
            "( GEOMETRIC_REPRESENTATION_CONTEXT(3) \
             GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty})) \
             GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length},#{angle},#{solid_angle})) \
             REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY') )"
        ));
        let items: Vec<String> = std::iter::once(origin)
            .chain(bodies.iter().copied())
            .map(|i| format!("#{i}"))
            .collect();
        let representation = self.add(format!("{shape}('',({}),#{context})", items.join(",")));
        let app = self.add("APPLICATION_CONTEXT('automotive design')".into());
        self.add(format!(
            "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"
        ));
        let product_context = self.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
        let product = self.add(format!(
            "PRODUCT({0},{0},'',(#{product_context}))",
            text(name)
        ));
        self.add(format!(
            "PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{product}))"
        ));
        let formation = self.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
        let definition_context = self.add(format!(
            "PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"
        ));
        let definition = self.add(format!(
            "PRODUCT_DEFINITION('design','',#{formation},#{definition_context})"
        ));
        let definition_shape = self.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
        self.add(format!(
            "SHAPE_DEFINITION_REPRESENTATION(#{definition_shape},#{representation})"
        ));
        if !styled.is_empty() {
            let items: Vec<String> = styled.iter().map(|i| format!("#{i}")).collect();
            self.add(format!(
                "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',({}),#{context})",
                items.join(",")
            ));
        }
        let mut out = String::from("ISO-10303-21;\nHEADER;\n");
        let _ = writeln!(out, "FILE_DESCRIPTION(({}),'2;1');", text(name));
        // A fixed time stamp: the same package always gives the same file.
        let _ = writeln!(
            out,
            "FILE_NAME({},'2000-01-01T00:00:00',(''),(''),'dutchman','dutchman','');",
            text(name)
        );
        out.push_str(
            "FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n",
        );
        for (i, line) in self.lines.iter().enumerate() {
            let _ = writeln!(out, "#{}={line};", i.saturating_add(1));
        }
        out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
        out
    }
}

/// An overlay as STEP: one open shell of planar faces at height `z`, one face per area (outer
/// loop counter-clockwise and hole loops clockwise, so the normal is +z), each coloured `rgb`.
fn sheet_step(
    name: &str,
    z: f64,
    areas: &[Area],
    rgb: [f64; 3],
    transparency: Option<f64>,
) -> String {
    let mut w = Step::default();
    let plane_placement = w.placement([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let plane = w.add(format!("PLANE('',#{plane_placement})"));
    let mut faces = Vec::new();
    for area in areas {
        let mut bounds = Vec::new();
        for (n, ring) in std::iter::once(&area.outer)
            .chain(area.holes.iter())
            .enumerate()
        {
            let at = |p: &[f64; 2]| [p[0], p[1], z];
            let vertices: Vec<usize> = ring.iter().map(|p| w.vertex(at(p))).collect();
            let mut edges = Vec::new();
            for (i, (p, q)) in ring.iter().zip(ring.iter().cycle().skip(1)).enumerate() {
                let next = (i + 1) % ring.len();
                if let (Some(&from), Some(&to)) = (vertices.get(i), vertices.get(next)) {
                    edges.push((w.edge(from, to, at(p), at(q)), true));
                }
            }
            let ring_loop = w.edge_loop(&edges);
            let kind = if n == 0 {
                "FACE_OUTER_BOUND"
            } else {
                "FACE_BOUND"
            };
            bounds.push(format!(
                "#{}",
                w.add(format!("{kind}('',#{ring_loop},.T.)"))
            ));
        }
        faces.push(w.add(format!(
            "ADVANCED_FACE('',({}),#{plane},.T.)",
            bounds.join(",")
        )));
    }
    let ids: Vec<String> = faces.iter().map(|f| format!("#{f}")).collect();
    let shell = w.add(format!("OPEN_SHELL('',({}))", ids.join(",")));
    let model = w.add(format!(
        "SHELL_BASED_SURFACE_MODEL({},(#{shell}))",
        text(name)
    ));
    finish_painted(
        w,
        name,
        "MANIFOLD_SURFACE_SHAPE_REPRESENTATION",
        &[model],
        &faces,
        rgb,
        transparency,
    )
}

/// The file for `bodies`, each of `faces` painted `rgb` (and, when given, with `transparency`).
fn finish_painted(
    mut w: Step,
    name: &str,
    shape: &str,
    bodies: &[usize],
    faces: &[usize],
    rgb: [f64; 3],
    transparency: Option<f64>,
) -> String {
    let colour = w.add(format!(
        "COLOUR_RGB('',{},{},{})",
        real(rgb[0]),
        real(rgb[1]),
        real(rgb[2])
    ));
    let fill = w.add(format!("FILL_AREA_STYLE_COLOUR('',#{colour})"));
    let fill_style = w.add(format!("FILL_AREA_STYLE('',(#{fill}))"));
    let surface_fill = w.add(format!("SURFACE_STYLE_FILL_AREA(#{fill_style})"));
    let styled: Vec<usize> = faces
        .iter()
        .map(|face| {
            // The transparency is written for each face beside its colour.
            let mut styles = format!("#{surface_fill}");
            if let Some(t) = transparency {
                let clear = w.add(format!("SURFACE_STYLE_TRANSPARENT({})", real(t)));
                styles.push_str(&format!(",#{clear}"));
            }
            let side = w.add(format!("SURFACE_SIDE_STYLE('',({styles}))"));
            let usage = w.add(format!("SURFACE_STYLE_USAGE(.BOTH.,#{side})"));
            let style = w.add(format!("PRESENTATION_STYLE_ASSIGNMENT((#{usage}))"));
            w.add(format!("STYLED_ITEM('',(#{style}),#{face})"))
        })
        .collect();
    w.finish(name, shape, bodies, &styled)
}

/// The most faces the thick overlays of one package may add together.
const MAX_SOLID_FACES: usize = 250_000;

/// The faces one area costs as a prism: top, bottom, and one side for each edge of every loop.
fn prism_faces(area: &Area) -> usize {
    area.holes
        .iter()
        .map(Vec::len)
        .fold(area.outer.len(), usize::saturating_add)
        .saturating_add(2)
}

/// An overlay as STEP with a thickness: one closed solid for each area, between heights `low`
/// and `high`, all in one shape (so one part, one look). Every prism has a top face (normal +z) and a bottom face (-z),
/// each bounded by the area's loops, and one planar side face for each loop edge with the
/// normal pointing out of the solid. Every face is coloured `rgb`.
fn solid_step(
    name: &str,
    low: f64,
    high: f64,
    areas: &[Area],
    rgb: [f64; 3],
    transparency: Option<f64>,
) -> String {
    let mut w = Step::default();
    let mut faces = Vec::new();
    let mut solids = Vec::new();
    for area in areas {
        let mut prism = Vec::new();
        let top_frame = w.placement([0.0, 0.0, high], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let top_plane = w.add(format!("PLANE('',#{top_frame})"));
        let bottom_frame = w.placement([0.0, 0.0, low], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
        let bottom_plane = w.add(format!("PLANE('',#{bottom_frame})"));
        let mut top_bounds = Vec::new();
        let mut bottom_bounds = Vec::new();
        for (n, ring) in std::iter::once(&area.outer)
            .chain(area.holes.iter())
            .enumerate()
        {
            let count = ring.len();
            let up = |p: &[f64; 2]| [p[0], p[1], high];
            let down = |p: &[f64; 2]| [p[0], p[1], low];
            let top_v: Vec<usize> = ring.iter().map(|p| w.vertex(up(p))).collect();
            let bottom_v: Vec<usize> = ring.iter().map(|p| w.vertex(down(p))).collect();
            let mut top_e = Vec::new();
            let mut bottom_e = Vec::new();
            let mut rise_e = Vec::new();
            for (i, (p, q)) in ring.iter().zip(ring.iter().cycle().skip(1)).enumerate() {
                let next = (i + 1) % count;
                if let (Some(&t0), Some(&t1), Some(&b0), Some(&b1)) = (
                    top_v.get(i),
                    top_v.get(next),
                    bottom_v.get(i),
                    bottom_v.get(next),
                ) {
                    top_e.push(w.edge(t0, t1, up(p), up(q)));
                    bottom_e.push(w.edge(b0, b1, down(p), down(q)));
                }
                if let (Some(&b0), Some(&t0)) = (bottom_v.get(i), top_v.get(i)) {
                    rise_e.push(w.edge(b0, t0, down(p), up(p)));
                }
            }
            let kind = if n == 0 {
                "FACE_OUTER_BOUND"
            } else {
                "FACE_BOUND"
            };
            let top_loop = w.edge_loop(&top_e.iter().map(|e| (*e, true)).collect::<Vec<_>>());
            top_bounds.push(format!("#{}", w.add(format!("{kind}('',#{top_loop},.T.)"))));
            let bottom_loop = w.edge_loop(
                &bottom_e
                    .iter()
                    .rev()
                    .map(|e| (*e, false))
                    .collect::<Vec<_>>(),
            );
            bottom_bounds.push(format!(
                "#{}",
                w.add(format!("{kind}('',#{bottom_loop},.T.)"))
            ));
            for (i, (p, q)) in ring.iter().zip(ring.iter().cycle().skip(1)).enumerate() {
                let next = (i + 1) % count;
                let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
                let len = dx.hypot(dy);
                let frame = w.placement(
                    down(p),
                    [dy / len, -dx / len, 0.0],
                    [dx / len, dy / len, 0.0],
                );
                let plane = w.add(format!("PLANE('',#{frame})"));
                if let (Some(&b), Some(&r1), Some(&t), Some(&r0)) = (
                    bottom_e.get(i),
                    rise_e.get(next),
                    top_e.get(i),
                    rise_e.get(i),
                ) {
                    let side_loop = w.edge_loop(&[(b, true), (r1, true), (t, false), (r0, false)]);
                    let bound = w.add(format!("FACE_OUTER_BOUND('',#{side_loop},.T.)"));
                    prism.push(w.add(format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)")));
                }
            }
        }
        prism.push(w.add(format!(
            "ADVANCED_FACE('',({}),#{top_plane},.T.)",
            top_bounds.join(",")
        )));
        prism.push(w.add(format!(
            "ADVANCED_FACE('',({}),#{bottom_plane},.T.)",
            bottom_bounds.join(",")
        )));
        // One solid for each area: a shell holding two disconnected lumps is read with the
        // later lumps as voids, which takes their volume away.
        let ids: Vec<String> = prism.iter().map(|f| format!("#{f}")).collect();
        let shell = w.add(format!("CLOSED_SHELL('',({}))", ids.join(",")));
        let label = if areas.len() > 1 {
            format!("{name} ({})", solids.len().saturating_add(1))
        } else {
            name.to_owned()
        };
        solids.push(w.add(format!("MANIFOLD_SOLID_BREP({},#{shell})", text(&label))));
        faces.extend(prism);
    }
    finish_painted(
        w,
        name,
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        &solids,
        &faces,
        rgb,
        transparency,
    )
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A solid box of the given size with a corner at the origin, as STEP text.
    pub(crate) fn box_step(name: &str, size: [f64; 3]) -> String {
        let [x, y, z] = size;
        let v: [[f64; 3]; 8] = [
            [0.0, 0.0, 0.0],
            [x, 0.0, 0.0],
            [x, y, 0.0],
            [0.0, y, 0.0],
            [0.0, 0.0, z],
            [x, 0.0, z],
            [x, y, z],
            [0.0, y, z],
        ];
        // Faces as vertex loops, counter-clockwise seen from outside.
        let faces: [[usize; 4]; 6] = [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [1, 2, 6, 5],
            [2, 3, 7, 6],
            [3, 0, 4, 7],
        ];
        let mut w = Step::default();
        let vertices: Vec<usize> = v.iter().map(|p| w.vertex(*p)).collect();
        let mut edges: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        let mut face_ids = Vec::new();
        for face in faces {
            let mut loop_edges = Vec::new();
            for k in 0..4 {
                let (a, b) = (face[k], face[(k + 1) % 4]);
                let key = (a.min(b), a.max(b));
                let edge = *edges.entry(key).or_insert_with(|| {
                    w.edge(vertices[key.0], vertices[key.1], v[key.0], v[key.1])
                });
                loop_edges.push((edge, a < b));
            }
            let lp = w.edge_loop(&loop_edges);
            let bound = w.add(format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
            let (p0, p1, p2) = (v[face[0]], v[face[1]], v[face[2]]);
            let d1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let d2 = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
            let n = [
                d1[1] * d2[2] - d1[2] * d2[1],
                d1[2] * d2[0] - d1[0] * d2[2],
                d1[0] * d2[1] - d1[1] * d2[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let dl = (d1[0] * d1[0] + d1[1] * d1[1] + d1[2] * d1[2]).sqrt();
            let frame = w.placement(
                p0,
                [n[0] / len, n[1] / len, n[2] / len],
                [d1[0] / dl, d1[1] / dl, d1[2] / dl],
            );
            let plane = w.add(format!("PLANE('',#{frame})"));
            face_ids.push(w.add(format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)")));
        }
        let ids: Vec<String> = face_ids.iter().map(|f| format!("#{f}")).collect();
        let shell = w.add(format!("CLOSED_SHELL('',({}))", ids.join(",")));
        let brep = w.add(format!("MANIFOLD_SOLID_BREP({},#{shell})", text(name)));
        w.finish(name, "ADVANCED_BREP_SHAPE_REPRESENTATION", &[brep], &[])
    }

    fn package(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dutchman-package-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn square(size: f64) -> Value {
        json!([[0.0, 0.0], [size, 0.0], [size, size], [0.0, size]])
    }

    fn manifest(extra: Value) -> Value {
        let mut base = json!({
            "schema": PARTS_SCHEMA,
            "ok": true,
            "name": "Test board",
            "units": "mm",
            "models": [
                {"id": "board", "file": "board.step"},
                {"id": "cube", "file": "cube.step"},
            ],
            "parts": [
                {"name": "Board", "model": "board", "transform": [[1,0,0,0],[0,1,0,0],[0,0,1,0]], "notes": []},
                {"name": "C1", "model": "cube", "transform": [[1,0,0,10],[0,1,0,5],[0,0,1,1.6]], "notes": []},
                {"name": "C2", "model": "cube", "transform": [[0,-1,0,20],[1,0,0,5],[0,0,1,1.6]], "notes": [
                    {"severity": "Degraded", "message": "turned"}
                ]},
            ],
            "overlays": [],
            "notes": [{"severity": "Info", "message": "made by a test"}],
        });
        if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in extra {
                base.insert(k.clone(), v.clone());
            }
        }
        base
    }

    fn write_models(dir: &Path) {
        std::fs::write(dir.join("board.step"), box_step("Board", [40.0, 20.0, 1.6])).unwrap();
        std::fs::write(dir.join("cube.step"), box_step("Cube", [2.0, 1.0, 0.5])).unwrap();
    }

    fn read(dir: &Path, manifest: &Value) -> (Result<Model, Error>, Report) {
        let mut report = Report::default();
        let model = read_package(dir, manifest.to_string().as_bytes(), &mut report);
        (model, report)
    }

    fn mentions(report: &Report, severity: Severity, needle: &str) -> bool {
        report.losses().iter().any(|l| {
            l.severity == severity && format!("{} {}", l.subject, l.detail).contains(needle)
        })
    }

    #[test]
    fn parts_are_placed_by_name_and_transform_with_one_definition_per_model() {
        let dir = package("place");
        write_models(&dir);
        let (model, report) = read(&dir, &manifest(json!({})));
        let model = model.unwrap();
        assert_eq!(model.name, "Test board");
        let assembly = model.assembly.unwrap();
        // Two models, three placements: the cube is one part placed twice.
        assert_eq!(assembly.parts.len(), 2);
        let names: Vec<_> = assembly
            .placements
            .iter()
            .map(|p| p.name.clone().unwrap())
            .collect();
        assert_eq!(names, ["Board", "C1", "C2"]);
        assert_eq!(assembly.placements[1].part, assembly.placements[2].part);
        assert_eq!(assembly.placements[1].transform[0][3], 10.0);
        assert_eq!(assembly.placements[2].transform[0][1], -1.0);
        assert_eq!(assembly.placements[2].transform[1][3], 5.0);
        assert_eq!(
            assembly.identities[assembly.placements[1].part],
            Some(format!("{PARTS_SCHEMA}:cube"))
        );
        assert!(assembly
            .parts
            .iter()
            .all(|p| !p.ir.geometry.bodies.is_empty()));
        assert!(mentions(&report, Severity::Info, "made by a test"));
        assert!(mentions(&report, Severity::Degraded, "turned"));
        assert!(report.completeness().is_some_and(|c| c.is_complete()));
        // The package states every part's place, so no model is placed by a guess.
        assert!(report.placement_assumptions().is_empty());
        assert!(report.part_positions().is_none());
        assert!(report
            .losses()
            .iter()
            .all(|l| !l.subject.ends_with("part positions")));
    }

    #[test]
    fn units_scale_the_translation() {
        let dir = package("units");
        write_models(&dir);
        let (model, _) = read(&dir, &manifest(json!({"units": "in"})));
        let assembly = model.unwrap().assembly.unwrap();
        assert!((assembly.placements[1].transform[0][3] - 254.0).abs() < 1e-9);
        let (refused, _) = read(&dir, &manifest(json!({"units": "furlong"})));
        assert!(refused.is_err());
    }

    #[test]
    fn two_conversions_are_identical() {
        let dir = package("twice");
        write_models(&dir);
        let overlay = json!({"overlays": [{
            "name": "Copper top", "kind": "copper", "finish": "masked", "side": "top",
            "part": "Board", "z": 1.601,
            "areas": [{"outer": square(5.0), "holes": []}],
        }]});
        let m = manifest(overlay);
        let summary = |model: Model, report: &Report| {
            let assembly = model.assembly.unwrap();
            (
                assembly
                    .placements
                    .iter()
                    .map(|p| (p.name.clone(), p.part, p.transform))
                    .collect::<Vec<_>>(),
                assembly
                    .parts
                    .iter()
                    .map(|p| (p.name.clone(), p.source_step.clone()))
                    .collect::<Vec<_>>(),
                assembly.identities,
                report
                    .losses()
                    .iter()
                    .map(|l| format!("{:?}|{}|{}", l.severity, l.subject, l.detail))
                    .collect::<Vec<_>>(),
            )
        };
        let (a, ra) = read(&dir, &m);
        let (b, rb) = read(&dir, &m);
        assert_eq!(summary(a.unwrap(), &ra), summary(b.unwrap(), &rb));
    }

    #[test]
    fn a_path_leaving_the_package_folder_is_refused() {
        let dir = package("escape");
        write_models(&dir);
        for file in [
            "../outside.step",
            "/etc/passwd",
            "a/../../b.step",
            "..\\x.step",
            "",
        ] {
            let mut m = manifest(json!({}));
            m["models"][1]["file"] = json!(file);
            let (model, _) = read(&dir, &m);
            assert!(
                matches!(model, Err(Error::Assembly(ref why)) if why.contains("refused")),
                "{file}: {model:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_package_folder_is_refused() {
        let dir = package("link");
        let outside = package("link-outside");
        std::fs::write(outside.join("far.step"), box_step("Far", [1.0, 1.0, 1.0])).unwrap();
        write_models(&dir);
        std::os::unix::fs::symlink(&outside, dir.join("elsewhere")).unwrap();
        let mut m = manifest(json!({}));
        m["models"][1]["file"] = json!("elsewhere/far.step");
        assert!(read(&dir, &m).0.is_err());
    }

    #[test]
    fn a_missing_step_file_is_a_named_dropped_loss_and_the_rest_converts() {
        let dir = package("missing");
        std::fs::write(dir.join("board.step"), box_step("Board", [40.0, 20.0, 1.6])).unwrap();
        let (model, report) = read(&dir, &manifest(json!({})));
        let assembly = model.unwrap().assembly.unwrap();
        let names: Vec<_> = assembly
            .placements
            .iter()
            .map(|p| p.name.clone().unwrap())
            .collect();
        assert_eq!(names, ["Board"]);
        assert!(mentions(&report, Severity::Dropped, "cube.step"));
        assert!(mentions(&report, Severity::Dropped, "C1"));
        assert!(mentions(&report, Severity::Dropped, "C2"));
        assert_eq!(assembly.left_out.len(), 2);
        assert!(!report.completeness().is_some_and(|c| c.is_complete()));
    }

    #[test]
    fn a_file_that_is_not_step_is_dropped_by_name() {
        let dir = package("garbage");
        write_models(&dir);
        std::fs::write(dir.join("cube.step"), b"not a step file").unwrap();
        let (model, report) = read(&dir, &manifest(json!({})));
        assert_eq!(model.unwrap().assembly.unwrap().placements.len(), 1);
        assert!(mentions(&report, Severity::Dropped, "model \"cube\""));
    }

    #[test]
    fn a_package_with_nothing_placeable_is_refused() {
        let dir = package("empty");
        let (model, _) = read(&dir, &manifest(json!({})));
        assert!(matches!(model, Err(Error::Assembly(_))));
    }

    #[test]
    fn wrong_schema_and_caps_are_refused() {
        let dir = package("caps");
        write_models(&dir);
        let (model, _) = read(&dir, &manifest(json!({"schema": "dutchman.parts/2"})));
        assert!(matches!(model, Err(Error::Assembly(ref why)) if why.contains("update")));
        let many: Vec<Value> = (0..=MAX_PARTS)
            .map(|i| json!({"id": format!("m{i}"), "file": "cube.step"}))
            .collect();
        assert!(read(&dir, &manifest(json!({"models": many}))).0.is_err());
        let mut report = Report::default();
        let big = vec![b' '; MAX_MANIFEST_BYTES + 1];
        assert!(read_package(&dir, &big, &mut report).is_err());
    }

    fn overlay(name: &str, areas: Value) -> Value {
        json!({
            "name": name, "kind": "copper", "finish": "masked", "side": "top",
            "part": "Board", "z": 1.601, "areas": areas,
        })
    }

    #[test]
    fn overlays_become_one_coloured_sheet_each_placed_with_their_part() {
        let dir = package("overlays");
        write_models(&dir);
        let hole = json!([[1.0, 1.0], [1.0, 4.0], [4.0, 4.0], [4.0, 1.0]]);
        let m = manifest(json!({"overlays": [
            overlay("Copper top", json!([
                {"outer": square(5.0), "holes": [hole]},
                {"outer": [[10.0, 10.0], [12.0, 10.0], [12.0, 12.0]], "holes": []},
            ])),
            {
                "name": "Silk top", "kind": "silkscreen", "side": "top", "part": "C1",
                "z": 0.51, "color": "#102030",
                "areas": [{"outer": square(1.0), "holes": []}],
            },
        ]}));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        // The board, the cube, and one part for each overlay.
        assert_eq!(assembly.parts.len(), 4);
        let copper = assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some("Copper top"))
            .unwrap();
        let board = &assembly.placements[0];
        assert_eq!(copper.transform, board.transform);
        let part = &assembly.parts[copper.part];
        assert_eq!(part.name, "Copper top");
        assert_eq!(
            part.ir.geometry.bodies.len(),
            1,
            "one sheet body per overlay"
        );
        assert_eq!(part.ir.geometry.faces.len(), 2, "one face per area");
        let step = part.source_step.as_deref().unwrap();
        // The two areas: outer plus a hole, and a triangle.
        assert_eq!(step.matches("FACE_OUTER_BOUND").count(), 2);
        assert_eq!(step.matches("FACE_BOUND(").count(), 1);
        assert!(step.contains("1.601"));
        // Default masked copper is solder mask green.
        assert!(step.contains("COLOUR_RGB('',0.05,0.45,0.22)"));
        // The silkscreen is placed with C1's transform, coloured from `color`.
        let silk = assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some("Silk top"))
            .unwrap();
        assert_eq!(silk.transform, assembly.placements[1].transform);
        let step = assembly.parts[silk.part].source_step.as_deref().unwrap();
        assert!(step.contains("COLOUR_RGB('',0.062745098,0.125490196,0.188235294)"));
        assert!(report
            .losses()
            .iter()
            .all(|l| l.severity != Severity::Dropped));
    }

    #[test]
    fn default_colours_by_kind_and_finish() {
        let green = sheet_step("a", 0.0, &[], MASKED_COPPER, None);
        assert!(green.contains("0.05,0.45,0.22"));
        let dir = package("colors");
        write_models(&dir);
        let mut bare = overlay("Bare", json!([{"outer": square(2.0), "holes": []}]));
        bare["finish"] = json!("bare");
        let mut silk = overlay("Silk", json!([{"outer": square(2.0), "holes": []}]));
        silk["kind"] = json!("silkscreen");
        silk.as_object_mut().unwrap().remove("finish");
        let (model, _) = read(&dir, &manifest(json!({"overlays": [bare, silk]})));
        let assembly = model.unwrap().assembly.unwrap();
        let step = |at: usize| assembly.parts[at].source_step.clone().unwrap();
        assert!(step(2).contains("COLOUR_RGB('',0.83,0.64,0.22)"));
        assert!(
            step(3).contains("COLOUR_RGB('',1.,1.,1.)")
                || step(3).contains("COLOUR_RGB('',1.0,1.0,1.0)")
        );
    }

    #[test]
    fn a_bad_area_is_dropped_with_a_degraded_note_naming_the_overlay() {
        let dir = package("badarea");
        write_models(&dir);
        let bowtie = json!([[0.0, 0.0], [4.0, 0.0], [1.0, 3.0], [4.0, 3.0]]);
        let through_edge = json!([[3.0, 1.0], [3.0, 3.0], [6.0, 3.0], [6.0, 1.0]]);
        let clockwise = json!([[0.0, 0.0], [0.0, 3.0], [3.0, 3.0], [3.0, 0.0]]);
        let nan_area = json!({"outer": [[0.0, 0.0], [1.0, 0.0], [1.0, null]], "holes": []});
        let m = manifest(json!({"overlays": [overlay("Copper top", json!([
            {"outer": square(5.0), "holes": []},
            {"outer": bowtie, "holes": []},
            {"outer": square(5.0), "holes": [through_edge]},
            {"outer": clockwise, "holes": []},
            {"outer": [[0.0, 0.0], [1.0, 0.0]], "holes": []},
            nan_area,
            {"outer": square(5.0), "holes": [[[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]]]},
        ]))]}));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        let part = assembly.parts.last().unwrap();
        assert_eq!(
            part.ir.geometry.faces.len(),
            1,
            "only the good area is drawn"
        );
        assert!(mentions(&report, Severity::Degraded, "Copper top"));
        assert!(mentions(&report, Severity::Degraded, "6 area(s) left out"));
        assert!(mentions(&report, Severity::Degraded, "crosses or touches"));
        assert!(mentions(&report, Severity::Degraded, "two loops cross"));
        assert!(mentions(&report, Severity::Degraded, "clockwise"));
        assert!(mentions(&report, Severity::Degraded, "fewer than 3"));
        // A hole that runs the wrong way is a bad area too.
        assert!(mentions(
            &report,
            Severity::Degraded,
            "hole 1 runs counter-clockwise"
        ));
    }

    #[test]
    fn an_overlay_naming_no_part_is_dropped() {
        let dir = package("nopart");
        write_models(&dir);
        let mut ghost = overlay("Ghost", json!([{"outer": square(1.0), "holes": []}]));
        ghost["part"] = json!("Nowhere");
        let mut unnamed = overlay("Unnamed", json!([{"outer": square(1.0), "holes": []}]));
        unnamed.as_object_mut().unwrap().remove("part");
        let mut odd = overlay("Odd", json!([{"outer": square(1.0), "holes": []}]));
        odd["kind"] = json!("soldermask");
        let (model, report) = read(&dir, &manifest(json!({"overlays": [ghost, unnamed, odd]})));
        assert_eq!(model.unwrap().assembly.unwrap().parts.len(), 2);
        assert!(mentions(&report, Severity::Dropped, "overlay \"Ghost\""));
        assert!(mentions(&report, Severity::Dropped, "overlay \"Unnamed\""));
        assert!(mentions(&report, Severity::Dropped, "overlay \"Odd\""));
    }

    #[test]
    fn an_overlay_on_a_part_that_was_left_out_is_dropped() {
        let dir = package("overlaydropped");
        std::fs::write(dir.join("cube.step"), box_step("Cube", [1.0, 1.0, 1.0])).unwrap();
        let (model, report) = read(
            &dir,
            &manifest(
                json!({"overlays": [overlay("Copper top", json!([{"outer": square(1.0), "holes": []}]))]}),
            ),
        );
        assert!(model.is_ok());
        assert!(mentions(
            &report,
            Severity::Dropped,
            "overlay \"Copper top\""
        ));
    }

    fn one_area_overlay(areas: Value) -> Value {
        manifest(json!({"overlays": [overlay("Copper top", areas)]}))
    }

    #[test]
    fn a_comb_ring_passes_the_work_limit_and_the_area_is_refused_by_name() {
        let dir = package("comb");
        write_models(&dir);
        let teeth = 4_990;
        let mut ring = vec![json!([0.0, 0.0])];
        for k in 0..teeth {
            let y = f64::from(2 * k);
            ring.push(json!([1000.0, y]));
            ring.push(json!([1000.0, y + 1.0]));
            ring.push(json!([1.0, y + 1.0]));
            ring.push(json!([1.0, y + 2.0]));
        }
        ring.push(json!([0.0, f64::from(2 * teeth)]));
        assert!(ring.len() <= MAX_RING_POINTS);
        let m = one_area_overlay(json!([
            {"outer": ring, "holes": []},
            {"outer": square(5.0), "holes": []},
        ]));
        let started = std::time::Instant::now();
        let (model, report) = read(&dir, &m);
        assert!(started.elapsed().as_secs() < 30);
        let assembly = model.unwrap().assembly.unwrap();
        assert!(mentions(&report, Severity::Degraded, "work limit"));
        assert!(mentions(&report, Severity::Degraded, "area 1"));
        // Placements still convert; the overlay survives only if an area was drawable.
        assert!(assembly.placements.len() >= 3);
    }

    #[test]
    fn a_ring_over_the_point_limit_is_refused() {
        let points: Vec<Value> = (0..=MAX_RING_POINTS)
            .map(|i| json!([f64::from(i as u32) * 0.001, 0.0]))
            .collect();
        let area = AreaEntry {
            outer: serde_json::from_value(json!(points)).unwrap(),
            holes: Vec::new(),
        };
        let err = validate_area(area, 1.0, &mut 1_000).unwrap_err();
        assert!(err.contains("the limit is"), "{err}");
    }

    #[test]
    fn a_hole_inside_another_hole_is_refused_by_name() {
        let dir = package("holehole");
        write_models(&dir);
        let big = json!([[2.0, 2.0], [2.0, 12.0], [12.0, 12.0], [12.0, 2.0]]);
        let small = json!([[4.0, 4.0], [4.0, 6.0], [6.0, 6.0], [6.0, 4.0]]);
        let m = one_area_overlay(json!([
            {"outer": square(20.0), "holes": [big, small]},
            {"outer": square(5.0), "holes": []},
        ]));
        let (model, report) = read(&dir, &m);
        assert!(model.is_ok());
        assert!(mentions(
            &report,
            Severity::Degraded,
            "hole 2 is inside hole 1"
        ));
    }

    #[test]
    fn the_first_part_of_a_duplicate_name_is_the_one_an_overlay_follows() {
        let dir = package("dupname");
        write_models(&dir);
        let mut m = manifest(json!({"overlays": [overlay(
            "Copper top",
            json!([{"outer": square(5.0), "holes": []}]),
        )]}));
        m["parts"].as_array_mut().unwrap().push(json!(
            {"name": "Board", "model": "board", "transform": [[1,0,0,77],[0,1,0,0],[0,0,1,0]], "notes": []}
        ));
        let (model, _) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        let copper = assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some("Copper top"))
            .unwrap();
        assert_eq!(copper.transform[0][3], 0.0);
    }

    #[test]
    fn a_shift_that_overflows_once_scaled_is_left_out_by_name() {
        let dir = package("overflow");
        write_models(&dir);
        let mut m = manifest(json!({"units": "m"}));
        m["parts"][1]["transform"] = json!([[1, 0, 0, 1.0e308], [0, 1, 0, 0], [0, 0, 1, 0]]);
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert!(mentions(&report, Severity::Dropped, "part \"C1\""));
        assert!(mentions(
            &report,
            Severity::Dropped,
            "not finite once scaled"
        ));
        assert!(assembly.placements.iter().all(|p| p
            .transform
            .iter()
            .flatten()
            .all(|v| v.is_finite())));
    }

    #[test]
    fn a_rotation_that_is_not_rigid_is_left_out_by_name() {
        let dir = package("notrigid");
        write_models(&dir);
        let mut m = manifest(json!({}));
        m["parts"][1]["transform"] = json!([[2, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0]]);
        m["parts"][2]["transform"] = json!([[1, 0.5, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0]]);
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert_eq!(assembly.placements.len(), 1);
        assert!(mentions(&report, Severity::Dropped, "part \"C1\""));
        assert!(mentions(&report, Severity::Dropped, "part \"C2\""));
        assert!(mentions(&report, Severity::Dropped, "not a rigid"));
    }

    #[test]
    fn near_coincident_points_are_dropped_and_a_ring_left_too_small_is_refused_alone() {
        let dir = package("near");
        write_models(&dir);
        let m = one_area_overlay(json!([
            {"outer": [[0.0, 0.0], [5.0, 0.0], [5.0, 1e-8], [5.0, 5.0], [0.0, 5.0], [1e-8, 5.0]],
             "holes": []},
            {"outer": [[0.0, 0.0], [5.0, 0.0], [5.0, 1e-8]], "holes": []},
        ]));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        let sheet = assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some("Copper top"))
            .expect("the good area is drawn");
        let step = assembly.parts[sheet.part].source_step.as_deref().unwrap();
        assert!(!step.contains("NaN") && !step.contains("inf"));
        assert!(mentions(&report, Severity::Degraded, "1 area(s) left out"));
        assert!(mentions(
            &report,
            Severity::Degraded,
            "fewer than 3 different"
        ));
        assert!(report
            .losses()
            .iter()
            .all(|l| l.severity != Severity::Dropped));
    }

    #[test]
    fn overlay_sheets_count_toward_the_part_limit() {
        let dir = package("partcap");
        write_models(&dir);
        let models: Vec<Value> = (0..MAX_PARTS - 1)
            .map(|i| json!({"id": format!("m{i}"), "file": "cube.step"}))
            .collect();
        let parts: Vec<Value> = (0..MAX_PARTS - 1)
            .map(|i| json!({"name": format!("P{i}"), "model": format!("m{i}"), "notes": []}))
            .collect();
        let sheet = |name: &str| {
            let mut o = overlay(name, json!([{"outer": square(1.0), "holes": []}]));
            o["part"] = json!("P0");
            o
        };
        let m = manifest(json!({
            "models": models, "parts": parts, "overlays": [sheet("S1"), sheet("S2")],
        }));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert_eq!(assembly.parts.len(), MAX_PARTS);
        assert!(mentions(&report, Severity::Dropped, "overlay \"S2\""));
        assert!(mentions(&report, Severity::Dropped, "parts"));
    }

    #[test]
    fn too_many_overlays_are_refused() {
        let dir = package("manyoverlays");
        write_models(&dir);
        let list: Vec<Value> = (0..=MAX_OVERLAYS)
            .map(|i| overlay(&format!("O{i}"), json!([])))
            .collect();
        let (model, _) = read(&dir, &manifest(json!({"overlays": list})));
        assert!(matches!(model, Err(Error::Assembly(ref why)) if why.contains("overlays")));
    }

    #[test]
    fn too_many_areas_drop_that_overlay_only() {
        let dir = package("manyareas");
        write_models(&dir);
        let areas: Vec<Value> = (0..=MAX_AREAS).map(|_| json!(null)).collect();
        let m = manifest(json!({"overlays": [
            overlay("Huge", json!(areas)),
            overlay("Fine", json!([{"outer": square(1.0), "holes": []}])),
        ]}));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert!(mentions(&report, Severity::Dropped, "overlay \"Huge\""));
        assert!(assembly
            .placements
            .iter()
            .any(|p| p.name.as_deref() == Some("Fine")));
    }

    #[test]
    fn windows_form_and_nul_file_names_are_refused() {
        let dir = package("winpaths");
        write_models(&dir);
        for file in [
            "C:\\x.step",
            "\\\\server\\share\\x.step",
            "a\\b.step",
            "a\0b.step",
        ] {
            let mut m = manifest(json!({}));
            m["models"][1]["file"] = json!(file);
            let (model, _) = read(&dir, &m);
            assert!(
                matches!(model, Err(Error::Assembly(ref why)) if why.contains("refused")),
                "{file:?}: {model:?}"
            );
        }
    }

    fn sheet_faces(assembly: &Assembly, name: &str) -> usize {
        let sheet = assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no sheet {name}"));
        assembly.parts[sheet.part].ir.geometry.faces.len()
    }

    #[test]
    fn loops_touching_at_one_vertex_are_accepted_even_within_rounding() {
        let dir = package("touch");
        write_models(&dir);
        let corner = json!([[0.0, 0.0], [1.0, 2.0], [2.0, 1.0]]);
        let a = json!([[2.0, 2.0], [2.0, 4.0], [4.0, 4.0], [4.0, 2.0]]);
        let b = json!([[4.000_000_1, 4.0], [5.0, 6.0], [6.0, 5.0]]);
        let m = one_area_overlay(json!([
            {"outer": square(10.0), "holes": [corner, a, b]},
        ]));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert_eq!(sheet_faces(&assembly, "Copper top"), 1);
        assert!(!mentions(&report, Severity::Degraded, "area(s) left out"));
        assert!(report
            .losses()
            .iter()
            .all(|l| l.severity != Severity::Dropped));
    }

    #[test]
    fn an_island_inside_another_areas_hole_is_its_own_face() {
        let dir = package("island");
        write_models(&dir);
        let hole = json!([[1.0, 1.0], [1.0, 4.0], [4.0, 4.0], [4.0, 1.0]]);
        let island = json!([[2.0, 2.0], [3.0, 2.0], [3.0, 3.0], [2.0, 3.0]]);
        let m = one_area_overlay(json!([
            {"outer": square(5.0), "holes": [hole]},
            {"outer": island, "holes": []},
        ]));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert_eq!(sheet_faces(&assembly, "Copper top"), 2);
        assert!(!mentions(&report, Severity::Degraded, "area(s) left out"));
        assert!(report
            .losses()
            .iter()
            .all(|l| l.severity != Severity::Dropped));
    }

    #[test]
    fn every_face_of_an_overlay_faces_the_same_way_and_takes_the_look_from_its_binding() {
        let dir = package("normals");
        write_models(&dir);
        let hole = json!([[1.0, 1.0], [1.0, 4.0], [4.0, 4.0], [4.0, 1.0]]);
        let island = json!([[2.0, 2.0], [3.0, 2.0], [3.0, 3.0], [2.0, 3.0]]);
        let m = one_area_overlay(json!([
            {"outer": square(5.0), "holes": [hole]},
            {"outer": island, "holes": []},
            {"outer": [[7.0, 7.0], [9.0, 7.0], [9.0, 9.0]], "holes": []},
        ]));
        let (model, _) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        let sheet = assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some("Copper top"))
            .unwrap();
        let part = &assembly.parts[sheet.part];
        let step = part.source_step.as_deref().unwrap();
        let faces: Vec<&str> = step
            .lines()
            .filter(|l| l.contains("ADVANCED_FACE"))
            .collect();
        assert_eq!(faces.len(), 3);
        // One plane, +z, and the same face sense for each.
        let tail = |l: &str| l.rsplit_once(",#").map(|(_, t)| t.to_owned()).unwrap();
        assert!(faces.iter().all(|f| tail(f) == tail(faces[0])), "{faces:?}");
        assert!(faces[0].ends_with(",.T.);"), "{faces:?}");
        // The look is the binding's alone: no face or body carries a colour of its own.
        assert!(part.ir.geometry.faces.iter().all(|f| f.color.is_none()));
        assert!(part.ir.geometry.bodies.iter().all(|b| b.color.is_none()));
        assert_eq!(part.ir.model.appearance_bindings.len(), 1);
    }

    #[test]
    fn a_model_every_part_paints_is_not_kept_unplaced() {
        let dir = package("unplaced");
        write_models(&dir);
        let mut m = manifest(json!({}));
        m["parts"][0]["color"] = json!("#c8b27a");
        let (model, _) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        for placement in &assembly.placements {
            assert!(placement.part < assembly.parts.len());
        }
        let used: std::collections::BTreeSet<_> =
            assembly.placements.iter().map(|p| p.part).collect();
        assert_eq!(used.len(), assembly.parts.len(), "a part no one places");
    }

    #[test]
    fn loops_sharing_part_of_an_edge_are_refused_by_name() {
        let dir = package("overlap");
        write_models(&dir);
        let along_the_edge = json!([[0.0, 2.0], [0.0, 5.0], [2.0, 5.0], [2.0, 2.0]]);
        let m = one_area_overlay(json!([
            {"outer": square(10.0), "holes": [along_the_edge]},
            {"outer": square(5.0), "holes": []},
        ]));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert_eq!(sheet_faces(&assembly, "Copper top"), 1);
        assert!(mentions(
            &report,
            Severity::Degraded,
            "two loops share part of an edge"
        ));
    }

    #[test]
    fn a_mask_overlay_is_drawn_with_its_holes_and_opacity() {
        let dir = package("mask");
        write_models(&dir);
        let pad = json!([[1.0, 1.0], [1.0, 2.0], [2.0, 2.0], [2.0, 1.0]]);
        let mut mask = overlay(
            "Mask top",
            json!([
                {"outer": square(5.0), "holes": [pad]},
                {"outer": [[6.0, 6.0], [8.0, 6.0], [8.0, 8.0]], "holes": []},
            ]),
        );
        mask["kind"] = json!("mask");
        mask.as_object_mut().unwrap().remove("finish");
        mask["opacity"] = json!(0.85);
        let mut plain = mask.clone();
        plain["name"] = json!("Mask plain");
        plain.as_object_mut().unwrap().remove("opacity");
        let mut bad = mask.clone();
        bad["name"] = json!("Mask bad");
        bad["opacity"] = json!(1.5);
        let (model, report) = read(&dir, &manifest(json!({"overlays": [mask, plain, bad]})));
        let assembly = model.unwrap().assembly.unwrap();
        let step_of = |name: &str| {
            let p = assembly
                .placements
                .iter()
                .find(|p| p.name.as_deref() == Some(name))
                .unwrap();
            assembly.parts[p.part].source_step.clone().unwrap()
        };
        let step = step_of("Mask top");
        assert!(step.contains("COLOUR_RGB('',0.05,0.45,0.22)"));
        assert!(step.contains("1.601"));
        assert_eq!(step.matches("FACE_BOUND(").count(), 1);
        assert_eq!(step.matches("SURFACE_STYLE_TRANSPARENT(0.15)").count(), 2);
        assert!(!step_of("Mask plain").contains("SURFACE_STYLE_TRANSPARENT"));
        assert!(mentions(&report, Severity::Dropped, "overlay \"Mask bad\""));
        assert!(mentions(&report, Severity::Dropped, "opacity"));
        assert!(!assembly
            .placements
            .iter()
            .any(|p| p.name.as_deref() == Some("Mask bad")));
    }

    fn step_of(assembly: &Assembly, name: &str) -> String {
        let p = assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no overlay {name}"));
        assembly.parts[p.part].source_step.clone().unwrap()
    }

    /// Every edge of the file is walked exactly twice, once each way: a closed manifold.
    fn assert_closed_manifold(step: &str) {
        let mut walks: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for line in step.lines() {
            let Some(at) = line.find("ORIENTED_EDGE('',*,*,#") else {
                continue;
            };
            let rest = &line[at + "ORIENTED_EDGE('',*,*,#".len()..];
            let (id, sense) = rest.split_once(',').unwrap();
            let entry = walks.entry(id.to_owned()).or_default();
            if sense.starts_with(".T.") {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
        assert!(!walks.is_empty());
        assert!(walks.values().all(|w| *w == (1, 1)), "{walks:?}");
    }

    fn thick(name: &str, side: &str, thickness: Value, areas: Value) -> Value {
        let mut o = overlay(name, areas);
        o["side"] = json!(side);
        o["thickness"] = thickness;
        o
    }

    fn holed() -> Value {
        json!([{"outer": square(5.0), "holes": [[[1.0, 1.0], [1.0, 4.0], [4.0, 4.0], [4.0, 1.0]]]}])
    }

    #[test]
    fn a_thick_overlay_is_one_closed_solid_with_a_prism_for_each_area() {
        let dir = package("thick");
        write_models(&dir);
        let m = manifest(json!({"overlays": [thick("Copper", "top", json!(0.035), holed())]}));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        let part = &assembly.parts[assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some("Copper"))
            .unwrap()
            .part];
        assert_eq!(part.ir.geometry.bodies.len(), 1, "one body");
        assert_eq!(part.ir.geometry.faces.len(), 2 + 4 + 4);
        let step = step_of(&assembly, "Copper");
        assert_eq!(step.matches("MANIFOLD_SOLID_BREP(").count(), 1);
        assert_eq!(step.matches("SHELL_BASED_SURFACE_MODEL").count(), 0);
        assert_closed_manifold(&step);
        // Top at z + t, bottom at z.
        assert!(step.contains("CARTESIAN_POINT('',(0.,0.,1.636))"));
        assert!(step.contains("CARTESIAN_POINT('',(0.,0.,1.601))"));
        assert!(step.contains("COLOUR_RGB('',0.05,0.45,0.22)"));
        assert!(report
            .losses()
            .iter()
            .all(|l| l.severity != Severity::Dropped));
    }

    #[test]
    fn two_areas_of_a_thick_overlay_are_two_solids_of_one_part() {
        let dir = package("lumps");
        write_models(&dir);
        let areas = json!([
            {"outer": square(2.0), "holes": []},
            {"outer": [[5.0, 5.0], [7.0, 5.0], [7.0, 7.0], [5.0, 7.0]], "holes": []},
        ]);
        let (model, _) = read(
            &dir,
            &manifest(json!({"overlays": [thick("Lumps", "top", json!(0.1), areas)]})),
        );
        let assembly = model.unwrap().assembly.unwrap();
        let part = &assembly.parts[assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some("Lumps"))
            .unwrap()
            .part];
        assert_eq!(part.ir.geometry.bodies.len(), 2, "one solid for each area");
        assert_eq!(part.ir.geometry.faces.len(), 12);
        assert_closed_manifold(&step_of(&assembly, "Lumps"));
    }

    #[test]
    fn a_bottom_overlay_extrudes_downward() {
        let dir = package("bottom");
        write_models(&dir);
        let m = manifest(
            json!({"overlays": [thick("Copper bottom", "bottom", json!(0.035), holed())]}),
        );
        let (model, _) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        let step = step_of(&assembly, "Copper bottom");
        assert!(step.contains("CARTESIAN_POINT('',(0.,0.,1.601))"));
        assert!(step.contains("CARTESIAN_POINT('',(0.,0.,1.566))"));
        assert!(!step.contains("1.636"));
        assert_closed_manifold(&step);
    }

    #[test]
    fn a_thickness_out_of_range_refuses_that_overlay_by_name() {
        let dir = package("thick-range");
        write_models(&dir);
        let m = manifest(json!({"overlays": [
            thick("Too thick", "top", json!(1.5), holed()),
            thick("Negative", "top", json!(-0.1), holed()),
            thick("Fine", "top", json!(1.0), holed()),
        ]}));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert!(mentions(
            &report,
            Severity::Dropped,
            "overlay \"Too thick\""
        ));
        assert!(mentions(&report, Severity::Dropped, "overlay \"Negative\""));
        assert!(mentions(&report, Severity::Dropped, "thickness"));
        let names: Vec<_> = assembly
            .placements
            .iter()
            .filter_map(|p| p.name.as_deref())
            .collect();
        assert!(!names.contains(&"Too thick") && !names.contains(&"Negative"));
        assert!(names.contains(&"Fine"));
    }

    #[test]
    fn over_the_face_budget_an_overlay_is_a_sheet_with_a_note() {
        let dir = package("thick-budget");
        write_models(&dir);
        // Each square prism costs 6 faces: 42,000 of them pass 250,000.
        let areas: Vec<Value> = (0..42_000u32)
            .map(|i| {
                let (x, y) = (f64::from(i % 200) * 2.0, f64::from(i / 200) * 2.0);
                json!({"outer": [[x, y], [x + 1.0, y], [x + 1.0, y + 1.0], [x, y + 1.0]], "holes": []})
            })
            .collect();
        let m = manifest(json!({"overlays": [
            thick("Huge", "top", json!(0.035), Value::Array(areas)),
            thick("Small", "top", json!(0.035), holed()),
        ]}));
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert!(mentions(&report, Severity::Degraded, "overlay \"Huge\""));
        assert!(mentions(&report, Severity::Degraded, "drawn as a sheet"));
        let step = step_of(&assembly, "Huge");
        assert!(step.contains("OPEN_SHELL") && !step.contains("MANIFOLD_SOLID_BREP"));
        // The budget is the package's: a later small overlay still gets its solid.
        assert!(step_of(&assembly, "Small").contains("MANIFOLD_SOLID_BREP"));
    }

    #[test]
    fn a_missing_or_zero_thickness_is_the_sheet_as_before() {
        let dir = package("thick-none");
        write_models(&dir);
        let mut zero = thick("Zero", "top", json!(0.0), holed());
        zero["thickness"] = json!(0);
        let (model, report) = read(
            &dir,
            &manifest(json!({"overlays": [overlay("Plain", holed()), zero]})),
        );
        let assembly = model.unwrap().assembly.unwrap();
        for name in ["Plain", "Zero"] {
            let step = step_of(&assembly, name);
            assert!(step.contains("OPEN_SHELL"), "{name}");
            assert!(!step.contains("MANIFOLD_SOLID_BREP"), "{name}");
            assert_eq!(sheet_faces(&assembly, name), 1);
        }
        assert!(!mentions(&report, Severity::Dropped, "overlay"));
        assert!(!mentions(&report, Severity::Degraded, "thickness"));
    }

    fn appearance_properties(assembly: &Assembly, name: &str) -> Vec<BTreeMap<String, f64>> {
        let p = assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no placement {name}"));
        let model = &assembly.parts[p.part].ir.model;
        // The appearances a binding names; the STEP text may carry unused ones.
        let used: BTreeSet<String> = serde_json::to_value(&model.appearance_bindings)
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["appearance"].as_str().map(str::to_owned))
            .collect();
        model
            .appearances
            .iter()
            .filter(|a| used.contains(a.id.as_str()))
            .map(|a| serde_json::from_value(serde_json::to_value(&a.properties).unwrap()).unwrap())
            .collect()
    }

    #[test]
    fn metallic_and_roughness_reach_the_appearance_and_missing_ones_add_nothing() {
        let dir = package("surface");
        write_models(&dir);
        let mut copper = overlay("Copper", holed());
        copper["metallic"] = json!(0.0);
        copper["roughness"] = json!(0.45);
        let mut mask = overlay("Mask", holed());
        mask["kind"] = json!("mask");
        mask["roughness"] = json!(0.35);
        let plain = overlay("Plain", holed());
        let mut m = manifest(json!({"overlays": [copper, mask, plain]}));
        m["parts"][0]["color"] = json!("#c8b27a");
        m["parts"][0]["metallic"] = json!(1.0);
        m["parts"][0]["roughness"] = json!(0.2);
        m["parts"][1]["color"] = json!("#336699");
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        let want = |m: f64, r: f64| {
            vec![BTreeMap::from([
                ("metallic".to_owned(), m),
                ("roughness".to_owned(), r),
            ])]
        };
        assert_eq!(appearance_properties(&assembly, "Copper"), want(0.0, 0.45));
        // Roughness alone: metallic 0.
        assert_eq!(appearance_properties(&assembly, "Mask"), want(0.0, 0.35));
        assert_eq!(
            appearance_properties(&assembly, "Plain"),
            vec![BTreeMap::new()]
        );
        let board = &assembly.placements[0];
        let board_props: Vec<_> = assembly.parts[board.part]
            .ir
            .model
            .appearances
            .iter()
            .map(|a| a.properties.len())
            .collect();
        assert_eq!(board_props, [2]);
        // A part with a colour and nothing else has none.
        let cube = &assembly.placements[1];
        assert!(assembly.parts[cube.part]
            .ir
            .model
            .appearances
            .iter()
            .all(|a| a.properties.is_empty()));
        assert!(report
            .losses()
            .iter()
            .all(|l| l.severity != Severity::Dropped));
    }

    #[test]
    fn metallic_or_roughness_out_of_range_refuses_that_overlay_or_part_by_name() {
        let dir = package("surface-range");
        write_models(&dir);
        let mut bad = overlay("Shiny", holed());
        bad["metallic"] = json!(1.5);
        let mut worse = overlay("Rough", holed());
        worse["roughness"] = json!(-0.1);
        let mut m = manifest(json!({"overlays": [bad, worse, overlay("Fine", holed())]}));
        m["parts"][1]["roughness"] = json!(2);
        m["parts"][1]["color"] = json!("#336699");
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert!(mentions(&report, Severity::Dropped, "overlay \"Shiny\""));
        assert!(mentions(&report, Severity::Dropped, "metallic"));
        assert!(mentions(&report, Severity::Dropped, "overlay \"Rough\""));
        assert!(mentions(&report, Severity::Dropped, "roughness"));
        let names: Vec<_> = assembly
            .placements
            .iter()
            .filter_map(|p| p.name.as_deref())
            .collect();
        assert!(names.contains(&"Fine"));
        assert!(!names.contains(&"Shiny") && !names.contains(&"Rough"));
        assert!(!names.contains(&"C1"), "{names:?}");
    }

    #[test]
    fn a_colour_that_is_not_rrggbb_refuses_that_overlay_or_part_by_name() {
        let dir = package("colors2");
        write_models(&dir);
        let mut m = manifest(json!({"overlays": [
            {"name": "Silk", "kind": "silkscreen", "part": "Board", "z": 1.6, "color": "yellow",
             "areas": [{"outer": square(1.0), "holes": []}]},
            overlay("Copper top", json!([{"outer": square(1.0), "holes": []}])),
        ]}));
        m["parts"][1]["color"] = json!("#336699");
        m["parts"][2]["color"] = json!("blue");
        let (model, report) = read(&dir, &m);
        let assembly = model.unwrap().assembly.unwrap();
        assert!(mentions(&report, Severity::Dropped, "overlay \"Silk\""));
        assert!(mentions(&report, Severity::Dropped, "part \"C2\""));
        assert!(mentions(&report, Severity::Dropped, "not #rrggbb"));
        let c1 = assembly
            .placements
            .iter()
            .find(|p| p.name.as_deref() == Some("C1"))
            .unwrap();
        let board = &assembly.placements[0];
        let part = &assembly.parts[c1.part];
        assert!(!part.ir.geometry.bodies.is_empty());
        assert_eq!(
            part.ir.model.appearance_bindings.len(),
            part.ir.geometry.bodies.len(),
            "each body of the part carries its color"
        );
        assert!(assembly.parts[board.part]
            .ir
            .model
            .appearance_bindings
            .is_empty());
        assert!(assembly
            .placements
            .iter()
            .any(|p| p.name.as_deref() == Some("Copper top")));
        assert!(!assembly
            .placements
            .iter()
            .any(|p| p.name.as_deref() == Some("C2")));
    }

    #[test]
    fn step_text_escapes_names() {
        assert_eq!(text("a'b\\c"), "'a''b\\\\c'");
        assert_eq!(text("é"), "'\\X2\\00E9\\X0\\'");
    }

    #[test]
    fn colours_parse_only_as_rrggbb() {
        assert_eq!(parse_color("#ff0000"), Some([1.0, 0.0, 0.0]));
        assert_eq!(parse_color("ff0000"), None);
        assert_eq!(parse_color("#ff00"), None);
        assert_eq!(parse_color("#gg0000"), None);
    }
}
