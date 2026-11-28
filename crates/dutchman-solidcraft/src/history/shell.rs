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
        // No removed face: a closed hollow shell of its target body.
        Some("faces") if selection.get("value").and_then(Value::as_array).is_some_and(Vec::is_empty) => Vec::new(),
        found @ Some("native" | "unresolved") => return Err(super::unsupported_selection("face", found)),
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
        let names = super::modify::body_names(feature, selection, ctx)?;
        match names.as_slice() {
            [name] => Some(name.clone()),
            _ => return Err("shelling several bodies at once is not supported yet".to_owned()),
        }
    } else {
        default_target(feature, selection, ctx)?
    };
    if points.is_empty() {
        if body.is_none() {
            return Err("a closed shell's target body is not resolved".to_owned());
        }
        if !crate::VersionGate::ClosedShell.supported(ctx.target_version) {
            return Err(format!(
                "a shell that removes no face is written for SolidCraft 0.45 or newer (target {})",
                ctx.target_version.minor()
            ));
        }
    }
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

/// The body a shell that names none acts on. Its removed faces lie in an input state: the
/// state's one body, or else the one body the state records as holding every removed face,
/// resolved as a selection in that state. When it is the state's one body and the history
/// has named no body yet, none: SolidCraft shells its one body. Otherwise the unique earlier
/// named body. With several, SolidCraft needs an explicit body so an overlapping face cannot
/// select another.
fn default_target(
    feature: &Value,
    removed: &Value,
    ctx: &Context,
) -> Result<Option<String>, String> {
    if let Some((id, state)) = removed
        .pointer("/value/state")
        .and_then(Value::as_str)
        .and_then(|id| Some((id, ctx.row("feature_input_topologies", id)?)))
    {
        let held = state
            .get("bodies")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let owner = match held {
            [body] => Some(body),
            _ => face_owner(removed, state),
        };
        if let Some(body) = owner {
            let selection = json!({"kind": "historical", "value": {"state": id, "bodies": [body]}});
            if let Ok(names) = super::modify::body_names(feature, &selection, ctx) {
                if let [name] = names.as_slice() {
                    return Ok(Some(name.clone()));
                }
            }
            if held.len() == 1 && ctx.named_bodies.is_empty() && ctx.generated_bodies.is_empty() {
                return Ok(None);
            }
        }
    }
    match ctx.named_bodies.as_slice() {
        [(_, name)] => Ok(Some(name.clone())),
        _ => Err("its shell target is not a unique earlier named body".to_owned()),
    }
}

/// The one body an input state records as holding every selected face.
fn face_owner<'a>(removed: &Value, state: &'a Value) -> Option<&'a Value> {
    let owners = state.get("face_owners").and_then(Value::as_array)?;
    let mut found: Option<&Value> = None;
    for face in removed.pointer("/value/faces").and_then(Value::as_array)? {
        let body = owners
            .iter()
            .find(|owner| owner.get("face") == Some(face))?
            .get("body")?;
        if found.is_some_and(|known| known != body) {
            return None;
        }
        found = Some(body);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn faces(state: &str, faces: &[&str]) -> Value {
        json!({"kind": "historical", "value": {"state": state, "faces": faces, "native": "n"}})
    }

    #[test]
    fn a_shell_without_bodies_targets_the_body_holding_its_faces() {
        let model = json!({
            "features": [{"id": "f:a", "outputs": ["a"]}, {"id": "f:b", "outputs": ["b"]}],
            "feature_result_topologies": [
                {"id": "ra", "output_of": "f:a", "members": [{"kind": "body", "id": "1"}]},
                {"id": "rb", "output_of": "f:b", "members": [{"kind": "body", "id": "1"}]}],
            "feature_input_topologies": [
                {"id": "one", "input_of": "shell", "bodies": ["one:1"],
                    "body_writers": [{"body": "one:1", "writer": "f:b"}]},
                {"id": "two", "input_of": "shell", "bodies": ["two:1", "two:2"],
                    "face_owners": [{"face": "top", "body": "two:2"}, {"face": "side", "body": "two:2"},
                        {"face": "other", "body": "two:1"}],
                    "body_writers": [{"body": "two:1", "writer": "f:a"}, {"body": "two:2", "writer": "f:b"}]}]
        });
        let mut ctx = Context::testing(&model);
        ctx.feature_names.insert("f:a".into(), "A".into());
        ctx.feature_names.insert("f:b".into(), "B".into());
        ctx.named_bodies
            .extend([("a".into(), "Body A".into()), ("b".into(), "Body B".into())]);
        let shell = json!({"id": "shell"});
        let target = |removed: &Value, ctx: &Context| default_target(&shell, removed, ctx);
        assert_eq!(
            target(&faces("one", &["top"]), &ctx).unwrap().as_deref(),
            Some("Body B")
        );
        assert_eq!(
            target(&faces("two", &["top", "side"]), &ctx)
                .unwrap()
                .as_deref(),
            Some("Body B")
        );
        // Faces on two bodies name no one target.
        assert!(target(&faces("two", &["top", "other"]), &ctx).is_err());
        // A state's one body the history could not name: SolidCraft shells its only body.
        let mut model = model.clone();
        model["feature_input_topologies"][0]["body_writers"] = json!([]);
        let ctx = Context::testing(&model);
        assert_eq!(target(&faces("one", &["top"]), &ctx).unwrap(), None);
    }

    #[test]
    fn an_empty_face_set_is_a_closed_shell_of_its_target_body() {
        let model = json!({});
        let mut ctx = Context::testing(&model);
        ctx.named_bodies.push(("cube".into(), "Cube".into()));
        let definition = |bodies: Value| {
            json!({"removed_faces": {"kind": "faces", "value": []},
            "bodies": bodies, "thickness": 1.0, "outward": false})
        };
        let shell = map(
            &json!({"id": "shell"}),
            &definition(json!({"kind": "bodies", "value": ["cube"]})),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(shell["faces"], json!([]));
        assert_eq!(shell["body"], "Cube");
        // A closed shell needs its body even when the history names none.
        let model = json!({});
        let mut ctx = Context::testing(&model);
        assert!(map(&json!({"id": "shell"}), &definition(Value::Null), &mut ctx).is_err());
    }
}
