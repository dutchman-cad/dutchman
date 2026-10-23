//! Sheet-metal base flanges: a sketch profile made a sheet of even thickness, as SolidCraft's
//! base flange makes it, under a sheet rule of that thickness. Hems are refused with the
//! reason SolidCraft cannot express them.

use serde_json::{json, Value};

use super::profile::profile;
use super::sketch::number;
use super::{name, Context};

/// Maps a base flange whose operation is `definition`.
pub(super) fn base_flange(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let (sketch, selection) = profile(definition.get("profile"), ctx)?;
    let sketch = ctx
        .sketches
        .get(&sketch)
        .map(|mapped| mapped.feature)
        .ok_or("its sketch is not an earlier mapped sketch")?;
    let thickness = number(definition.get("thickness"))
        .filter(|t| *t > 0.0)
        .ok_or("its thickness is not given")?;
    // SolidCraft grows the sheet along the sketch's normal, or against it when flipped.
    let flip = match definition.get("side").and_then(Value::as_str) {
        None | Some("forward") => false,
        Some("reverse") => true,
        Some(_) => {
            return Err(
                "a base flange thick on both sides of its sketch is not mapped yet".to_owned(),
            )
        }
    };
    let rule = ctx.sheet_rule(thickness);
    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("BaseFlange{id}")),
        "type": "sheet_base",
        "suppressed": false,
        "sketch": sketch,
        "profiles": selection,
        "rule": rule,
    });
    if let (true, Some(fields)) = (flip, out.as_object_mut()) {
        fields.insert("flip".to_owned(), Value::Bool(true));
    }
    Ok(out)
}

/// CADIR hems retain developed lengths; SolidCraft accepts an outer-face length.
pub(super) fn hem_refusal(definition: &Value) -> String {
    let reason = match definition.get("direction").and_then(Value::as_str) {
        Some("unresolved") => "a hem with unresolved fold direction is not supported yet",
        _ => match definition.pointer("/form/kind").and_then(Value::as_str) {
            Some("flat" | "open") => {
                "a hem specified by developed length is not supported yet; SolidCraft requires length to the outer face"
            }
            Some("gap_length") => "an unresolved flat-or-open hem is not supported yet",
            Some("rolled") => "a rolled hem is not supported yet",
            Some("teardrop") => "a teardrop hem is not supported yet",
            _ => "this hem form is not supported yet",
        },
    };
    reason.to_owned()
}

/// A SolidCraft sheet rule of `thickness` mm, named after it, with SolidCraft's own defaults
/// for everything else.
pub(super) fn rule(name: &str, thickness: f64) -> Value {
    json!({
        "name": name,
        "thickness": format!("{thickness} mm"),
        "bend_radius": "Thickness",
        "k_factor": "0.44",
        "relief_width": "Thickness",
        "relief_depth": "Thickness * 0.5",
        "corner_relief": "Thickness * 4.0",
        "corner_relief_shape": "notch",
        "hem_gap": "0.01 mm",
        "gap": "Thickness",
    })
}
