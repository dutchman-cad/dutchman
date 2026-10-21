//! Lofts, as SolidCraft loft features.
//!
//! A loft through sketch profiles maps when SolidCraft builds the same shape from it: open,
//! solid, smooth (not ruled), with no degree limit, each section a profile of its own sketch,
//! and guided by at most two rails of sketch curves (not by a centreline). A loft that closes to a point is not mapped: SolidCraft wants
//! the point as a sketch point, and CADIR gives a position or a vertex.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::extrude::{body_names, operation};
use super::profile::profile;
use super::sweep::curves;
use super::{name, Context};

/// Maps a loft feature whose operation is `definition`.
pub(crate) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
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
    if !flag("solid", true) {
        return Err("surface lofts are not mapped yet".to_owned());
    }
    if flag("ruled", false) {
        return Err("ruled lofts are not mapped yet".to_owned());
    }
    if definition.get("max_degree").is_some_and(|d| !d.is_null()) {
        return Err("lofts with a degree limit are not mapped yet".to_owned());
    }

    let sources = definition
        .get("sections")
        .and_then(Value::as_array)
        .ok_or("its sections are malformed")?;
    if sources.len() < 2 {
        return Err("a loft needs two sections".to_owned());
    }
    let mut sections = Vec::with_capacity(sources.len());
    let mut sketches = BTreeSet::new();
    for section in sources {
        if matches!(
            section.get("kind").and_then(Value::as_str),
            Some("point" | "native_point" | "vertex")
        ) {
            return Err("lofts that close to a point are not mapped yet".to_owned());
        }
        let (sketch_id, profiles) = profile(Some(section), ctx)?;
        let sketch = ctx
            .sketches
            .get(&sketch_id)
            .ok_or("a section's sketch is not an earlier mapped sketch")?;
        if !sketches.insert(sketch.feature) {
            return Err("loft sections that share a sketch are not mapped yet".to_owned());
        }
        sections.push(json!({"sketch": sketch.feature, "profiles": profiles}));
    }
    let operation = operation(definition.get("op"))?;
    let body_names = body_names(feature, ctx, operation);

    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Loft{id}")),
        "type": "loft",
        "suppressed": false,
        "sections": sections,
        "operation": operation,
    });
    if let Some(fields) = out.as_object_mut() {
        if !rails.is_empty() {
            fields.insert("rails".to_owned(), json!(rails));
        }
        if let Some(names) = body_names {
            fields.insert("body_names".to_owned(), json!(names));
        }
    }
    Ok(out)
}
