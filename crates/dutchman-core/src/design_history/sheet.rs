//! Sheet-metal walls: the base flange of a sheet-metal body.

use serde_json::{json, Map, Value};

use super::json::{kind, number, payload};
use super::ops::{profile, refuse_stated, Built, Note};
use super::sketch::Sketches;
use super::{Plan, CODE_SHEET_RULE};

/// The sheet-metal operation as the model-history base flange. Only a wall is projected.
pub(super) fn project(
    operation: &Value,
    plan: &Plan,
    sketches: &Sketches<'_>,
) -> Result<Built, String> {
    refuse_stated(
        operation,
        &["kind", "native_options", "source_settings"],
        &[],
    )?;
    let wall = operation.get("kind").unwrap_or(&Value::Null);
    if kind(wall) != Some("wall") {
        return Err(format!(
            "a {} sheet-metal feature is not projected yet",
            kind(wall).unwrap_or("malformed")
        ));
    }
    let wall = payload(wall);
    refuse_stated(wall, &["kind", "profile", "thickness", "side", "rule"], &[])?;
    let (sketch, carrier) = profile(wall.get("profile"), plan, sketches)?;
    let mut row = Map::new();
    // A base flange names its profile without the planar wrapper an extrude puts around it.
    row.insert(
        "profile".to_owned(),
        json!({"kind": "sketch", "value": sketch}),
    );
    if let Some(thickness) = wall.get("thickness") {
        let thickness = number(thickness)?;
        if thickness <= 0.0 {
            return Err("its thickness is not positive".to_owned());
        }
        row.insert("thickness".to_owned(), json!(thickness));
    }
    // The row reads a side it does not state as the side along the normal, so the design has
    // to state one.
    let side = match wall.get("side").and_then(Value::as_str) {
        Some("along") => "forward",
        Some("against") => "reverse",
        Some("symmetric") => "symmetric",
        Some(_) => return Err("its thickness side is malformed".to_owned()),
        None => return Err("it states no side of the profile its thickness lies on".to_owned()),
    };
    row.insert("side".to_owned(), json!(side));
    let mut built = Built::new(
        json!({"kind": "sheet_metal_base_flange", "value": row}),
        vec![carrier.to_owned()],
    );
    if wall.get("rule").is_some() {
        built.notes.push(Note {
            code: CODE_SHEET_RULE,
            message: "the wall names a sheet-metal rule; its thickness is projected, and its bend radius and bend allowance are not".to_owned(),
        });
    }
    Ok(built)
}
