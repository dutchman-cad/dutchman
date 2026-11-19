//! Planar faces moved along their normal, as SolidCraft's offset-face feature.
//!
//! CADIR gives the faces as faces of the feature's own input topology, known by the geometry of
//! the edges around them ([`super::face_profile`]). SolidCraft picks a face by a point on it, on
//! the body as it rebuilds it before the feature, so each face is written as a point inside it.
//! Its offset is a distance along the outward normal, which is the motion CADIR calls an offset.
//! SolidCraft `solid.offset_face` has only scalar `distance`, not a `translate` vector.
//! Its `solid.move` command accepts `translate` but selects whole bodies, so neither face
//! translations nor rotations can use it. Both are refused.

use serde_json::{json, Map, Value};

use super::sketch::number;
use super::{face_profile, name, Context};

pub(super) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let motion = definition
        .get("motion")
        .ok_or("the motion of its faces is not given")?;
    let distance = match motion.get("kind").and_then(Value::as_str) {
        Some("offset") => {
            number(motion.get("distance")).ok_or("the distance its faces move is not resolved")?
        }
        Some("translate") => {
            return Err(
                "translating faces along a direction is not supported yet; only a move along \
                 the face normal is"
                    .to_owned(),
            )
        }
        Some("rotate") => return Err("rotating faces is not supported yet".to_owned()),
        _ => return Err("the motion of its faces is not resolved".to_owned()),
    };
    let selection = definition.get("faces").ok_or("its faces are not given")?;
    let points = match selection.get("kind").and_then(Value::as_str) {
        Some("historical") => face_profile::planar_points(
            feature,
            selection.get("value").unwrap_or(&Value::Null),
            ctx,
        )?,
        found @ Some("native" | "unresolved") => {
            return Err(super::unsupported_selection("face", found))
        }
        Some("historical_partial") => {
            return Err("some of its faces are not resolved in its input topology".to_owned())
        }
        _ => return Err("moved faces must come from the feature's own input topology".to_owned()),
    };
    let id = ctx.id();
    let mut out = Map::new();
    out.insert("type".to_owned(), json!("offset_face"));
    out.insert("suppressed".to_owned(), json!(false));
    out.insert("faces".to_owned(), json!(points));
    out.insert("distance".to_owned(), json!(format!("{distance} mm")));
    out.insert("id".to_owned(), json!(id));
    out.insert(
        "name".to_owned(),
        json!(name(feature)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("MoveFace{id}"))),
    );
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affine_face_moves_are_never_written_as_normal_offsets() {
        // SolidCraft 0.43 has OffsetFace (normal distance), but no face
        // translation or rotation. Preserve the refusal until a target can
        // express those motions; a body Move would move unrelated faces too.
        for transform in [
            json!([
                [1.0, 2e-31, 0.0, -7.0],
                [-2e-31, 1.0, 0.0, 3.0],
                [0.0, 0.0, 1.0, 0.0]
            ]),
            json!([
                [0.0, -1.0, 0.0, -7.0],
                [1.0, 0.0, 0.0, 3.0],
                [0.0, 0.0, 1.0, 0.0]
            ]),
        ] {
            let model = json!({});
            let mut ctx = Context::testing(&model);
            let definition = json!({"motion": {"kind": "transform", "transform": transform}});
            assert_eq!(
                map(&json!({}), &definition, &mut ctx),
                Err("the motion of its faces is not resolved".to_owned())
            );
        }
    }
}
