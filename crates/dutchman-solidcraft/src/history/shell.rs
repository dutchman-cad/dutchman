//! Shells selected on the feature's historical input, never on its final faces.

use serde_json::{json, Value};

use super::sketch::number;
use super::{face_profile, name, Context};

pub(super) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let selection = definition
        .get("removed_faces")
        .ok_or("its removed faces are not given")?;
    let points = match selection.get("kind").and_then(Value::as_str) {
        Some("historical") => face_profile::interior_points(feature,
            selection.get("value").unwrap_or(&Value::Null), ctx)?,
        Some("native" | "unresolved") => return Err("its shell input faces are not resolved yet".to_owned()),
        Some("historical_partial") => return Err("some shell input faces are not resolved yet".to_owned()),
        _ => return Err("shell requires faces from its own input topology; final-body faces are not supported yet".to_owned()),
    };
    let thickness = number(definition.get("thickness"))
        .filter(|v| *v > 0.0)
        .ok_or("its shell thickness is not resolved")?;
    let outward = definition
        .get("outward")
        .and_then(Value::as_bool)
        .ok_or("its shell direction is not resolved")?;
    if definition
        .get("mode")
        .is_some_and(|v| !v.is_null() && v.as_str() != Some("skin"))
    {
        return Err("this shell offset mode is not supported yet".to_owned());
    }
    if definition
        .get("join")
        .is_some_and(|v| !v.is_null() && v.as_str() != Some("intersection"))
    {
        return Err("this shell corner continuation is not supported yet".to_owned());
    }
    let body = if let Some(selection) = definition.get("bodies").filter(|v| !v.is_null()) {
        let names = super::modify::body_names(selection, ctx)?;
        match names.as_slice() {
            [name] => Some(name.clone()),
            _ => return Err("shelling several bodies at once is not supported yet".to_owned()),
        }
    } else {
        // SolidCraft can choose the unique earlier body. With several it needs an
        // explicit source scope so an overlapping face cannot select another body.
        match ctx.named_bodies.as_slice() {
            [(_, name)] => Some(name.clone()),
            _ => return Err("its shell target is not a unique earlier named body".to_owned()),
        }
    };
    let id = ctx.id();
    let mut out = json!({"id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Shell{id}")),
        "type": "shell", "suppressed": false, "faces": points,
        "thickness": format!("{thickness} mm"),
        "direction": if outward { "outside" } else { "inside" }, "tangent_chain": false});
    if let (Some(body), Some(fields)) = (body, out.as_object_mut()) {
        fields.insert("body".to_owned(), json!(body));
    }
    Ok(out)
}
