//! Construction helpers whose consumers already carry resolved geometry.

use serde_json::Value;

use super::{describe, operation, Context, Mapped};
use dutchman_core::{Report, Severity};

pub(super) fn is_helper(feature: &Value) -> bool {
    matches!(
        super::operation_kind(feature),
        Some("binder" | "datum_point" | "datum_coordinate_system" | "datum_axis" | "datum_plane")
    ) || (super::operation_kind(feature) == Some("native")
        && operation(feature)
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| {
                dutchman_core::is_native_binder(kind)
                    || dutchman_core::is_native_coordinate_system(kind)
            }))
}

/// A binder, or a native operation other than a coordinate frame: its source geometry must
/// resolve. A native coordinate frame only places later features.
fn is_binder_like(kind: &str, definition: &Value) -> bool {
    kind == "binder"
        || (kind == "native"
            && !definition
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(dutchman_core::is_native_coordinate_system))
}

fn contains(value: &Value, ids: &[&str]) -> bool {
    match value {
        Value::String(s) => ids.contains(&s.as_str()),
        Value::Array(values) => values.iter().any(|v| contains(v, ids)),
        Value::Object(fields) => fields.values().any(|v| contains(v, ids)),
        _ => false,
    }
}

pub(super) fn map(feature: &Value, ctx: &Context, report: &mut Report) -> Result<Mapped, String> {
    let id = feature
        .get("id")
        .and_then(Value::as_str)
        .ok_or("a helper has no identity")?;
    let mut ids = vec![id];
    ids.extend(
        feature
            .get("outputs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str),
    );
    let ordinal = feature.get("ordinal").and_then(Value::as_u64);
    let users: Vec<&Value> = ctx
        .arena("features")
        .iter()
        .filter(|other| {
            other.get("suppressed").and_then(Value::as_bool) != Some(true)
                && other.get("ordinal").and_then(Value::as_u64) > ordinal
                && (contains(other.get("dependencies").unwrap_or(&Value::Null), &ids)
                    || contains(operation(other), &ids))
        })
        .collect();
    if users.is_empty() {
        report.push(
            Severity::Info,
            describe(feature),
            "helper not converted: nothing uses it",
        );
        return Ok(Mapped::Nothing);
    }
    let definition = operation(feature);
    let kind = super::operation_kind(feature).unwrap_or_default();
    if is_binder_like(kind, definition) {
        // A construction plane has one carrier frame, already carried by attached sketches.
        // A native subelement selector on a solid does not prove which source face it means.
        let source = definition
            .get("sources")
            .and_then(Value::as_array)
            .filter(|sources| sources.len() == 1)
            .and_then(|sources| sources.first());
        let plane = source
            .and_then(|source| source.pointer("/target/feature"))
            .and_then(Value::as_str)
            .and_then(|id| ctx.row("features", id))
            .is_some_and(|source| {
                matches!(
                    super::operation_kind(source),
                    Some("datum_plane" | "datum_principal_plane")
                )
            });
        if kind == "binder"
            && plane
            && definition
                .pointer("/construction/kind")
                .and_then(Value::as_str)
                == Some("shape")
            && definition
                .pointer("/construction/trace_support")
                .and_then(Value::as_bool)
                == Some(false)
            && users.iter().all(|user| resolved_sketch(user, ctx))
        {
            report.push(
                Severity::Info,
                describe(feature),
                "helper not converted: its source plane is carried by the sketches that use it",
            );
            return Ok(Mapped::Nothing);
        }
        return Err("a binder used by a later feature is not mapped yet: its source geometry reference is not resolved to a supported plane or body selection".to_owned());
    }
    if kind == "datum_plane" && definition.get("frame").is_some_and(Value::is_object) {
        return Ok(Mapped::Nothing);
    }
    // These operations embed their placement or axes. A dependency edge alone needs no
    // target feature; an unresolved selection still needs the helper and must be refused.
    if users.iter().all(|user| match super::operation_kind(user) {
        Some("sketch") => resolved_sketch(user, ctx),
        Some("pattern") => !contains(operation(user), &ids) && !unresolved(operation(user)),
        Some("datum_plane" | "datum_coordinate_system") => {
            operation(user).get("frame").is_some_and(Value::is_object)
        }
        Some("datum_point") => operation(user)
            .get("position")
            .is_some_and(Value::is_object),
        Some("datum_axis") => {
            operation(user).get("origin").is_some_and(Value::is_object)
                && operation(user)
                    .get("direction")
                    .is_some_and(Value::is_object)
        }
        _ => false,
    }) {
        report.push(
            Severity::Info,
            describe(feature),
            "helper not converted: its resolved placement is carried by the features that use it",
        );
        Ok(Mapped::Nothing)
    } else {
        let name = match definition.get("kind").and_then(Value::as_str) {
            Some(native) if kind == "native" => format!("native {native}"),
            _ => kind.replace('_', " "),
        };
        Err(format!("a {name} used by a later feature is not mapped yet: the dependent feature does not carry its resolved placement"))
    }
}

pub(super) fn resolved_sketch(feature: &Value, ctx: &Context) -> bool {
    super::operation_kind(feature) == Some("sketch")
        && operation(feature)
            .pointer("/sketch/sketch")
            .and_then(Value::as_str)
            .and_then(|id| ctx.row("sketches", id))
            .is_some_and(|sketch| {
                sketch.pointer("/placement/kind").and_then(Value::as_str) == Some("resolved")
            })
}

fn unresolved(value: &Value) -> bool {
    match value {
        Value::Object(fields) => {
            fields
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "native" | "native_set" | "unresolved"))
                || fields.values().any(unresolved)
        }
        Value::Array(values) => values.iter().any(unresolved),
        _ => false,
    }
}

/// A number as text with float noise removed: 12 significant digits, trailing zeros trimmed,
/// so `30.000000000000004` is `30` and `0.1 + 0.2` is `0.3` on every platform.
pub(crate) fn clean(value: f64) -> String {
    let rounded = format!("{value:.11e}").parse::<f64>().unwrap_or(value);
    format!("{rounded}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use serde_json::json;

    fn helper(definition: Value) -> Value {
        json!({"id": "test:model:feature#helper", "ordinal": 1, "definition": {"kind": "operation", "value": definition}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []})
    }

    fn user(definition: Value) -> Value {
        json!({"id": "test:model:feature#user", "ordinal": 2, "dependencies": ["test:model:feature#helper"], "definition": {"kind": "operation", "value": definition}, "source_properties": {}, "source_content": [], "outputs": []})
    }

    #[test]
    fn unused_helpers_and_suppressed_users_do_not_block() {
        for definition in [
            json!({"kind": "datum_point", "value": {"position": {"x": 2, "y": 3, "z": 4}}}),
            json!({"kind": "datum_coordinate_system", "value": {}}),
            json!({"kind": "binder", "value": {}}),
            json!({"kind": "native", "value": {"kind": "PartDesign::SubShapeBinder", "parameters": {}}}),
            json!({"kind": "native", "value": {"kind": "PartDesign::ShapeBinder", "parameters": {}}}),
            json!({"kind": "native", "value": {"kind": "CoordinateSystem", "parameters": {}}}),
        ] {
            let helper = helper(definition);
            assert!(is_helper(&helper));
            let mut suppressed = user(json!({"kind": "combine", "value": {}}));
            suppressed["suppressed"] = json!(true);
            let wire = json!({"features": [helper, suppressed]});
            let ctx = Context::testing(&wire);
            let mut report = Report::default();
            assert!(matches!(
                map(&wire["features"][0], &ctx, &mut report),
                Ok(Mapped::Nothing)
            ));
            assert!(report
                .losses()
                .iter()
                .any(|loss| loss.severity == Severity::Info
                    && loss.detail == "helper not converted: nothing uses it"));
        }
    }

    #[test]
    fn a_used_native_coordinate_system_needs_users_that_carry_their_placement() {
        let frame = helper(
            json!({"kind": "native", "value": {"kind": "CoordinateSystem", "parameters": {}}}),
        );
        assert!(is_helper(&frame));
        let wire =
            json!({"features": [frame.clone(), user(json!({"kind": "combine", "value": {}}))]});
        let ctx = Context::testing(&wire);
        let refused = map(&wire["features"][0], &ctx, &mut Report::default())
            .err()
            .unwrap();
        assert!(
            refused.contains("does not carry its resolved placement"),
            "{refused}"
        );
        let wire = json!({"features": [frame, user(json!({"kind": "datum_point", "value": {"position": {"x": 1, "y": 2, "z": 3}}}))]});
        let ctx = Context::testing(&wire);
        assert!(matches!(
            map(&wire["features"][0], &ctx, &mut Report::default()),
            Ok(Mapped::Nothing)
        ));
    }

    #[test]
    fn references_to_helper_outputs_count_as_uses() {
        let mut helper = helper(
            json!({"kind": "native", "value": {"kind": "PartDesign::SubShapeBinder", "parameters": {}}}),
        );
        helper["outputs"] = json!(["test:model:body#bound"]);
        let wire = json!({"features": [helper, {"id": "test:model:feature#boolean", "ordinal": 2, "definition": {"kind": "operation", "value": {"kind": "combine", "value": {"tools": {"kind": "resolved", "value": ["test:model:body#bound"]}, "destination": "target_component"}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}]});
        assert!(map(
            &wire["features"][0],
            &Context::testing(&wire),
            &mut Report::default()
        )
        .is_err());
    }

    #[test]
    fn whole_plane_binders_resolve_to_the_source_plane_for_attached_sketches() {
        let binder = helper(
            json!({"kind": "binder", "value": {"sources": [{"target": {"kind": "feature", "feature": "plane"}}], "construction": {"kind": "shape", "trace_support": false}}}),
        );
        let wire = json!({"features": [
            {"id": "plane", "ordinal": 0, "definition": {"kind": "operation", "value": {"kind": "datum_plane", "value": {"frame": {}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []},
            binder, user(json!({"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": "s"}}}))],
            "sketches": [{"id": "s", "placement": {"kind": "resolved"}, "profiles": []}]});
        assert!(map(
            &wire["features"][1],
            &Context::testing(&wire),
            &mut Report::default()
        )
        .is_ok());
        let mut face = wire;
        face["features"][1]["definition"]["value"]["value"]["sources"][0]["subelements"] =
            json!(["Face1"]);
        assert!(map(
            &face["features"][1],
            &Context::testing(&face),
            &mut Report::default()
        )
        .is_ok());
        face["features"][0]["definition"]["value"]["kind"] = json!("extrude");
        assert!(map(
            &face["features"][1],
            &Context::testing(&face),
            &mut Report::default()
        )
        .is_err());
    }

    #[test]
    fn used_opaque_binders_remain_blockers() {
        for kind in ["binder", "native"] {
            let helper =
                helper(json!({"kind": kind, "value": {"kind": "PartDesign::SubShapeBinder"}}));
            for definition in [
                json!({"kind": "combine", "value": {}}),
                json!({"kind": "sketch", "value": {}}),
            ] {
                let wire = json!({"features": [helper, user(definition)]});
                assert!(map(
                    &wire["features"][0],
                    &Context::testing(&wire),
                    &mut Report::default()
                )
                .err()
                .unwrap()
                .contains("a binder used by a later feature is not mapped yet"));
            }
        }
    }

    #[test]
    fn resolved_sketches_carry_used_datum_placements() {
        for kind in ["datum_point", "datum_coordinate_system"] {
            for resolved in [true, false] {
                let wire = json!({"features": [helper(json!({"kind": kind, "value": {}})),
                    user(json!({"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": "s"}}}))],
                    "sketches": [{"id": "s", "placement": {"kind": if resolved {"resolved"} else {"unresolved"}}, "profiles": []}]});
                assert_eq!(
                    map(
                        &wire["features"][0],
                        &Context::testing(&wire),
                        &mut Report::default()
                    )
                    .is_ok(),
                    resolved
                );
            }
        }
    }

    #[test]
    fn resolved_datums_carry_the_helpers_they_depend_on() {
        let wire = json!({"features": [helper(json!({"kind": "datum_point", "value": {}})),
            user(json!({"kind": "datum_coordinate_system", "value": {"frame": {}}}))]});
        assert!(map(
            &wire["features"][0],
            &Context::testing(&wire),
            &mut Report::default()
        )
        .is_ok());
        let mut unresolved = wire;
        unresolved["features"][1]["definition"]["value"]["value"]["frame"] = Value::Null;
        assert!(map(
            &unresolved["features"][0],
            &Context::testing(&unresolved),
            &mut Report::default()
        )
        .is_err());
    }

    #[test]
    fn resolved_pattern_axes_need_no_datum_feature() {
        for kind in ["datum_point", "datum_coordinate_system"] {
            for pattern in [
                json!({"kind": "circular", "axis_origin": {"x": 0, "y": 0, "z": 0},
                "axis_dir": {"x": 0, "y": 0, "z": 1}, "angle": std::f64::consts::TAU, "count": 3}),
                json!({"kind": "unresolved", "native": "JointOrigin.Z"}),
            ] {
                let resolved = pattern["kind"] == "circular";
                let wire = json!({"features": [helper(json!({"kind": kind, "value": {}})),
                    user(json!({"kind": "pattern", "value": {"pattern": pattern}}))]});
                assert_eq!(
                    map(
                        &wire["features"][0],
                        &Context::testing(&wire),
                        &mut Report::default()
                    )
                    .is_ok(),
                    resolved
                );
            }
        }
    }
}

#[cfg(test)]
mod clean_tests {
    use super::clean;

    #[test]
    fn float_noise_is_removed_from_numbers() {
        assert_eq!(clean(30.000000000000004), "30");
        assert_eq!(clean(0.1 + 0.2), "0.3");
        assert_eq!(clean(1.5000000000000002), "1.5");
        assert_eq!(clean(-0.006000000000000001), "-0.006");
        assert_eq!(clean(0.0), "0");
        assert_eq!(clean(12.5), "12.5");
        assert_eq!(clean(1234567.0), "1234567");
    }
}
