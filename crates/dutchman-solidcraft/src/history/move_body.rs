//! Rigid body motion uses only CADIR's resolved transform and earlier body identities.
//! SolidCraft rotates about the world origin, then translates. Fold an explicit
//! CADIR pivot into the translation as t + p - R p.

use serde_json::{json, Value};

use super::sketch::{cross, dot, number, v3};
use super::{modify, name, Context};

pub(super) fn map(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    preceding: &[Value],
) -> Result<Value, String> {
    if !crate::VersionGate::MoveBody.supported(ctx.target_version) {
        return Err("body moves require SolidCraft 0.45 or newer; choose a newer target to keep this feature editable".into());
    }
    match definition.get("copies").and_then(Value::as_u64) {
        None | Some(0) => (),
        Some(_) => return Err("copied body result identities are not supported yet".into()),
    }
    let bodies = modify::body_names(
        feature,
        definition
            .get("bodies")
            .ok_or("its moved bodies are not given")?,
        ctx,
    )?;
    if bodies
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != bodies.len()
    {
        return Err("it selects one moved body twice".into());
    }
    // An import carrying `_bodies` restates final geometry. Its body is already
    // moved; only a captured historical import (`_step`) supplies an input here.
    if preceding.iter().any(|earlier| {
        earlier.get("_bodies").is_some()
            && earlier
                .get("body_names")
                .and_then(Value::as_array)
                .is_some_and(|names| {
                    names
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|name| bodies.iter().any(|body| body == name))
                })
    }) {
        return Err("its moved body is available only in the final geometry; moving it without a captured pre-move body is not supported yet".into());
    }
    let mut translation = v3(definition.get("translation"))
        .ok_or("its body translation is not resolved; unresolved point-to-point motion is not supported yet")?;
    let mut rotation_fields = None;
    if let Some(rotation) = definition.get("rotation").filter(|v| !v.is_null()) {
        let pivot = v3(rotation.get("origin")).ok_or("its rotation pivot is not resolved")?;
        let axis = v3(rotation.get("direction")).ok_or("its rotation axis is not resolved")?;
        let [x, y, z] = axis;
        let norm = x.hypot(y).hypot(z);
        if !norm.is_finite() || norm == 0.0 {
            return Err("its rotation axis has no finite direction".into());
        }
        let axis = axis.map(|v| v / norm);
        let angle = number(rotation.get("angle")).ok_or("its rotation angle is not resolved")?;
        let (sin, cos) = angle.sin_cos();
        let perpendicular = cross(axis, pivot);
        let parallel = dot(axis, pivot) * (1.0 - cos);
        for (((t, p), k), c) in translation
            .iter_mut()
            .zip(pivot)
            .zip(axis)
            .zip(perpendicular)
        {
            *t += p - (p * cos + c * sin + k * parallel);
        }
        rotation_fields = Some((axis, angle));
    }
    if translation.iter().any(|v| !v.is_finite()) {
        return Err("its body transform exceeds finite coordinates".into());
    }
    let id = ctx.id();
    let mut out = json!({
        "id": id, "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Move{id}")),
        "type": "move", "suppressed": false, "bodies": bodies,
        "translate": translation.map(|v| format!("{v} mm")), "copy": false,
    });
    if let (Some((axis, angle)), Some(fields)) = (rotation_fields, out.as_object_mut()) {
        fields.insert("rotate_axis".into(), json!(axis));
        fields.insert("angle".into(), json!(format!("{angle} rad")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_names_only_the_selected_earlier_bodies() {
        let model = json!({});
        let mut ctx = Context::testing(&model);
        ctx.named_bodies.extend([
            ("chosen".into(), "Chosen".into()),
            ("other".into(), "Other".into()),
        ]);
        let definition = json!({"bodies": {"kind": "bodies", "value": ["chosen"]},
            "translation": {"x": 3.0, "y": 0.0, "z": 0.0}});
        let moved = map(&json!({}), &definition, &mut ctx, &[]).unwrap();
        assert_eq!(moved["bodies"], json!(["Chosen"]));
        assert_eq!(ctx.named_bodies.len(), 2);
        let final_import =
            json!({"type": "import", "_bodies": ["chosen"], "body_names": ["Chosen"]});
        assert!(map(&json!({}), &definition, &mut ctx, &[final_import])
            .unwrap_err()
            .contains("pre-move body"));
        let captured = json!({"type": "import", "_step": "captured", "body_names": ["Chosen"]});
        assert!(map(&json!({}), &definition, &mut ctx, &[captured]).is_ok());
        let mut duplicated = definition;
        duplicated["bodies"]["value"] = json!(["chosen", "chosen"]);
        assert!(map(&json!({}), &duplicated, &mut ctx, &[])
            .unwrap_err()
            .contains("twice"));
    }

    #[test]
    fn unresolved_or_unrepresentable_motion_never_becomes_a_zero_move() {
        let model = json!({});
        let mut ctx = Context::testing(&model);
        ctx.named_bodies.push(("body".into(), "Block".into()));
        let base = json!({"bodies": {"kind": "bodies", "value": ["body"]},
            "translation": {"x": 0.0, "y": 0.0, "z": 0.0}});
        let mut missing = base.clone();
        missing.as_object_mut().unwrap().remove("translation");
        assert!(map(&json!({}), &missing, &mut ctx, &[])
            .unwrap_err()
            .contains("point-to-point"));
        for rotation in [
            json!({"origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 0.0}, "angle": 1.0}),
            json!({"direction": {"x": 0.0, "y": 0.0, "z": 1.0}, "angle": 1.0}),
            json!({"origin": {"x": f64::MAX, "y": f64::MAX, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0}, "angle": std::f64::consts::PI}),
        ] {
            let mut definition = base.clone();
            definition["rotation"] = rotation;
            assert!(map(&json!({}), &definition, &mut ctx, &[]).is_err());
        }
    }
}
