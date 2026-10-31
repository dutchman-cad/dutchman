//! Faces deleted from a body, as SolidCraft's delete-face feature.
//!
//! CADIR gives the faces as faces of the feature's own input topology, known by the geometry of
//! the edges around them ([`super::face_profile`]). SolidCraft picks a face by a point on it,
//! on the body as it rebuilds it before the feature, so each face is written as a point inside
//! it. A selection cadmpeg leaves native or unresolved is refused for that reason.

use serde_json::{json, Map, Value};

use super::{face_profile, name, Context};

pub(super) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let selection = definition.get("faces").ok_or("its faces are not given")?;
    let points = match selection.get("kind").and_then(Value::as_str) {
        Some("historical") => face_profile::interior_points(
            feature,
            selection.get("value").unwrap_or(&Value::Null),
            ctx,
        )?,
        Some("native" | "unresolved") => {
            return Err(
                "its faces are a selection cadmpeg does not resolve to faces yet".to_owned(),
            )
        }
        Some("historical_partial") => {
            return Err("some of its faces are not resolved in its input topology".to_owned())
        }
        _ => {
            return Err("deleted faces must come from the feature's own input topology".to_owned())
        }
    };
    let heal = definition
        .get("heal")
        .and_then(Value::as_bool)
        .ok_or("whether it heals the gap is not given")?;
    let id = ctx.id();
    let mut out = Map::new();
    out.insert("type".to_owned(), json!("delete_face"));
    out.insert("suppressed".to_owned(), json!(false));
    out.insert("faces".to_owned(), json!(points));
    out.insert("heal".to_owned(), json!(heal));
    out.insert("id".to_owned(), json!(id));
    out.insert(
        "name".to_owned(),
        json!(name(feature)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("DeleteFace{id}"))),
    );
    Ok(Value::Object(out))
}
