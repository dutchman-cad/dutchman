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
    let copy = match definition.get("copies").and_then(Value::as_u64) {
        None | Some(0) => false,
        Some(1) => true,
        Some(_) => {
            return Err("a move that makes several copies of a body is not supported yet".into())
        }
    };
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
    let copy_name = if copy {
        Some(copy_name(feature, &bodies, ctx)?)
    } else {
        None
    };
    let id = ctx.id();
    let mut out = json!({
        "id": id, "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Move{id}")),
        "type": "move", "suppressed": false, "bodies": bodies,
        "translate": translation.map(|v| format!("{v} mm")), "copy": copy,
    });
    if let (Some((output, copy_name)), Some(fields)) = (copy_name, out.as_object_mut()) {
        // SolidCraft names the copy by the feature's stored name when that name is free.
        fields.insert("body_names".into(), json!([copy_name]));
        ctx.named_bodies.push((output, copy_name));
    }
    if let (Some((axis, angle)), Some(fields)) = (rotation_fields, out.as_object_mut()) {
        fields.insert("rotate_axis".into(), json!(axis));
        fields.insert("angle".into(), json!(format!("{angle} rad")));
    }
    Ok(out)
}

/// The copy a move makes of its one selected body: the feature's one output that no earlier
/// feature named, under the source's own name for it when that name is free.
fn copy_name(
    feature: &Value,
    bodies: &[String],
    ctx: &Context,
) -> Result<(String, String), String> {
    let [source] = bodies else {
        return Err("copying several bodies in one move is not supported yet: CADIR does not pair each copy with its source".into());
    };
    let outputs: Vec<&str> = feature
        .get("outputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let [output] = outputs.as_slice() else {
        return Err("its copied body is not its one output".into());
    };
    if ctx.named_bodies.iter().any(|(id, _)| id == output) {
        return Err("its copied body is not its one output".into());
    }
    let base = ctx
        .row("bodies", output)
        .and_then(|row| row.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map_or_else(|| source.clone(), str::to_owned);
    let mut name = base.clone();
    let mut n = 2;
    while ctx.body_name_taken(&name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    Ok(((*output).to_owned(), name))
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
    fn a_copying_move_names_its_one_output() {
        let model = json!({});
        let mut ctx = Context::testing(&model);
        ctx.named_bodies.push(("body".into(), "Block".into()));
        let definition = json!({"bodies": {"kind": "bodies", "value": ["body"]},
            "copies": 1, "translation": {"x": 0.0, "y": 0.0, "z": 0.0}});
        let feature = json!({"outputs": ["copy"]});
        let moved = map(&feature, &definition, &mut ctx, &[]).unwrap();
        assert_eq!(moved["copy"], json!(true));
        assert_eq!(moved["bodies"], json!(["Block"]));
        assert_eq!(moved["body_names"], json!(["Block (2)"]));
        assert!(ctx
            .named_bodies
            .contains(&("copy".to_owned(), "Block (2)".to_owned())));
        // The output is the copy only when no earlier feature named it.
        let again = map(&feature, &definition, &mut ctx, &[]).unwrap_err();
        assert!(again.contains("not its one output"), "{again}");
        for (outputs, copies, reason) in [
            (json!([]), 1, "not its one output"),
            (json!(["a", "b"]), 1, "not its one output"),
            (json!(["fresh"]), 2, "several copies"),
        ] {
            let mut definition = definition.clone();
            definition["copies"] = json!(copies);
            let error = map(&json!({"outputs": outputs}), &definition, &mut ctx, &[]).unwrap_err();
            assert!(error.contains(reason), "{error}");
        }
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
