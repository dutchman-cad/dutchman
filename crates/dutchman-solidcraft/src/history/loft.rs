//! Lofts, as SolidCraft loft features.
//!
//! A loft through sketch profiles maps when SolidCraft builds the same shape from it: open,
//! with no degree limit, through two to fifty sections (several may be profiles of one
//! sketch), guided by at most two rails of sketch curves or one-curve whole sketches (not by
//! a centreline). A ruled loft maps through two sections without rails, where SolidCraft's loft
//! is ruled too. A surface loft becomes a SolidCraft surface loft (walls only, a new surface
//! body). A first or last section of a solid loft that is a point (a position or a vertex)
//! becomes a sketch holding that point, written just before the loft, since SolidCraft takes a
//! loft's point end as a sketch point. Edge and face simplification after the loft changes no
//! shape and is left to SolidCraft.

use serde_json::{json, Value};

use super::extrude::{body_names, operation};
use super::profile::profile;
use super::sketch::v3;
use super::sweep::curves;
use super::{name, Context};

/// Maps a loft feature whose operation is `definition`, appending any apex sketches
/// to `preceding` only once the complete loft has mapped.
pub(crate) fn map(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    preceding: &mut Vec<Value>,
) -> Result<Value, String> {
    let guidance = definition.get("guidance").unwrap_or(&Value::Null);
    let guides = match guidance.get("kind").and_then(Value::as_str) {
        None => &[][..],
        Some("guides") => guidance
            .get("path")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice),
        Some("centerline") => return Err("lofts along a centreline are not mapped yet".to_owned()),
        Some(_) => return Err("its guidance is malformed".to_owned()),
    };
    if guides.len() > 2 {
        return Err("lofts along more than two guide rails are not mapped yet".to_owned());
    }
    let rails = guides
        .iter()
        .map(|guide| {
            curves(Some(guide), ctx, "guide rail")
                .map(|(sketch, curves)| json!({"sketch": sketch, "curves": curves}))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let flag = |key: &str, default: bool| {
        definition
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    };
    if flag("closed", false) {
        return Err("closed lofts are not mapped yet".to_owned());
    }
    let surface = !flag("solid", true);
    let ruled = flag("ruled", false);
    if definition.get("max_degree").is_some_and(|d| !d.is_null()) {
        return Err("lofts with a degree limit are not mapped yet".to_owned());
    }

    let sources = definition
        .get("sections")
        .and_then(Value::as_array)
        .ok_or("its sections are malformed")?;
    if !(2..=50).contains(&sources.len()) {
        return Err("a loft needs between two and fifty sections".to_owned());
    }
    // SolidCraft rules a loft between two sections and runs one smooth surface through
    // three or more, so a ruled loft is the same shape only through two, without rails.
    if ruled && sources.len() > 2 {
        return Err("ruled lofts through more than two sections are not mapped yet".to_owned());
    }
    if ruled && !rails.is_empty() {
        return Err("ruled lofts along guide rails are not mapped yet".to_owned());
    }
    let mut sections = Vec::with_capacity(sources.len());
    let mut apex_sketches = Vec::new();
    for (index, section) in sources.iter().enumerate() {
        if matches!(
            section.get("kind").and_then(Value::as_str),
            Some("point" | "native_point" | "vertex")
        ) {
            if surface {
                return Err("surface lofts to a point are not mapped yet".to_owned());
            }
            if index != 0 && index + 1 != sources.len() {
                return Err("lofts with an interior point section are not mapped yet".to_owned());
            }
            let position = match section.get("kind").and_then(Value::as_str) {
                Some("point") => v3(section.get("value")),
                Some("vertex") => section
                    .get("value")
                    .and_then(Value::as_str)
                    .and_then(|id| ctx.row("vertices", id))
                    .and_then(|vertex| vertex.get("point"))
                    .and_then(Value::as_str)
                    .and_then(|id| ctx.row("points", id))
                    .and_then(|point| v3(point.get("position"))),
                _ => {
                    return Err(
                        "lofts that close to an unresolved point are not mapped yet".to_owned()
                    )
                }
            }
            .ok_or("a loft point section has no resolved position")?;
            let id = ctx.id();
            let apex_name = ctx.unique_name(
                &json!({"name": format!("Loft apex {id}")}),
                preceding.iter(),
            );
            apex_sketches.push(json!({
                "id": id,
                "name": apex_name,
                "type": "sketch",
                "suppressed": false,
                "plane": {"type": "custom", "plane": {
                    "origin": position,
                    "x_dir": [1.0, 0.0, 0.0],
                    "y_dir": [0.0, 1.0, 0.0],
                }},
                "sketch": {
                    "points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true}],
                    "curves": [],
                    "constraints": [],
                },
            }));
            sections.push(json!({"sketch": id, "point": "origin", "profiles": {"type": "all"}}));
            continue;
        }
        let (sketch_id, profiles) = profile(Some(section), ctx)?;
        let sketch = ctx
            .sketches
            .get(&sketch_id)
            .ok_or("a section's sketch is not an earlier mapped sketch")?;
        sections.push(json!({"sketch": sketch.feature, "profiles": profiles}));
    }
    // A surface loft is a sheet of its own; SolidCraft makes it a new surface body.
    let operation = if surface {
        "new_body"
    } else {
        operation(definition.get("op"))?
    };
    let body_names = body_names(feature, ctx, operation);

    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Loft{id}")),
        "type": if surface { "surface_loft" } else { "loft" },
        "suppressed": false,
        "sections": sections,
        "operation": operation,
    });
    if let Some(fields) = out.as_object_mut() {
        if surface {
            fields.remove("operation");
        }
        if !rails.is_empty() {
            fields.insert("rails".to_owned(), json!(rails));
        }
        if let Some(names) = body_names {
            fields.insert("body_names".to_owned(), json!(names));
        }
    }
    preceding.extend(apex_sketches);
    Ok(out)
}
