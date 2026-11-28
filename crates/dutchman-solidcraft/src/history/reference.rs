//! Neutral design references written using SolidCraft's Insert Design representation.
use dutchman_core::{Model, Report, Severity, Writer};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{name, Context};
use crate::{SolidCraftWriter, Verification, VersionGate};

/// The schema of the reference payload: the neutral design as the CADIR wire states it (`ir`),
/// the arenas the wire omits (`omitted`, the object `Model::omitted_arenas_value` writes),
/// the STEP text, the name and the body identities. The Onshape reader writes no other
/// schema, so an earlier one, which carried only the wire, is not read.
const REFERENCE_SCHEMA: &str = "dutchman.design-reference/2";

/// The referenced neutral design: the wire document with every arena of `omitted` restored
/// into its `model`, read through `ir_wire`, which types them again.
#[cfg(any(test, feature = "onshape"))]
fn neutral_design(reference: &Value) -> Result<cadmpeg_ir::CadIr, String> {
    let mut document = reference
        .get("ir")
        .cloned()
        .ok_or("the referenced neutral design is missing")?;
    let omitted = reference
        .get("omitted")
        .and_then(Value::as_object)
        .ok_or("the arenas of the referenced neutral design are missing")?;
    let model = document
        .get_mut("model")
        .and_then(Value::as_object_mut)
        .ok_or("the referenced neutral design has no model")?;
    for (arena, rows) in omitted {
        model.insert(arena.clone(), rows.clone());
    }
    dutchman_core::ir_wire::from_value(document)
        .map_err(|e| format!("the referenced neutral design cannot be read: {e}"))
}

/// Only Onshape input writes a design reference, and a build without it (the SolidCraft
/// plugin) does not link the parser for one.
#[cfg(not(any(test, feature = "onshape")))]
fn neutral_design(_reference: &Value) -> Result<cadmpeg_ir::CadIr, String> {
    Err("the referenced neutral design cannot be read in this build".into())
}

pub(super) fn map(
    feature: &Value,
    reference: &Value,
    ctx: &mut Context,
    report: &mut Report,
) -> Result<Value, String> {
    let reference = payload(reference)?;
    let source = Model {
        ir: neutral_design(&reference)?,
        source_step: Some(
            reference
                .get("step")
                .and_then(Value::as_str)
                .ok_or("the referenced geometry is missing")?
                .into(),
        ),
        merge_bodies: false,
        name: reference
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Referenced design")
            .into(),
        fidelity: None,
        source_file: None,
        assembly: None,
    };
    let bodies = reference
        .get("bodies")
        .and_then(Value::as_array)
        .filter(|b| !b.is_empty() && b.len() <= 10_000)
        .ok_or("the referenced body identities are missing")?;
    let mut names = Vec::new();
    let mut bindings = Vec::new();
    for body in bodies {
        let id = body
            .get("id")
            .and_then(Value::as_str)
            .ok_or("a referenced body identity is missing")?;
        let name = body
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("a referenced body name is missing")?;
        if names.iter().any(|n| n == name) || ctx.named_bodies.iter().any(|(_, n)| n == name) {
            return Err("a referenced body name collides with an earlier body; this insert is not supported yet".into());
        }
        names.push(name.to_owned());
        bindings.push((id.to_owned(), name.to_owned()));
    }
    let step = crate::step::restate(&source, &mut Report::default()).map_err(|e| e.to_string())?;
    let wire = dutchman_core::ir_wire::to_value(&source.ir).map_err(|e| e.to_string())?;
    let mut source_ctx = Context::new(wire.get("model").unwrap_or(&Value::Null))
        .with_geometry(wire.get("geometry").unwrap_or(&Value::Null));
    source_ctx.source = Some(&source);
    let source_ids: Vec<_> = source
        .ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect();
    let ordered_names = super::faces::import_names(&mut source_ctx, &source_ids, Some(&step));
    if ordered_names.len() != names.len() || names.iter().any(|name| !ordered_names.contains(name))
    {
        return Err("the referenced shapes cannot be matched uniquely to their saved names".into());
    }
    let id = ctx.id();
    let label = name(feature).unwrap_or("Insert Design");
    let mut mapped = json!({"id":id,"name":label,"type":"import","suppressed":false,
        "file":format!("{label}.step"),"step":"","_step":step,"body_names":ordered_names});
    if VersionGate::LinkedDesign.supported(ctx.target_version) {
        let mut bytes = Vec::new();
        let mut source_report = Report::default();
        SolidCraftWriter::new()
            .target_version(Some(ctx.target_version))
            .verification(Verification::Off)
            .write(&source, &mut bytes, &mut source_report)
            .map_err(|error| error.to_string())?;
        report.append_prefixed(source_report, label);
        let digest: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let fields = mapped
            .as_object_mut()
            .ok_or("the linked Import fields are missing")?;
        fields.insert("file".into(), json!(format!("derived-{digest}.sc")));
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        fields.insert("_linked_design".into(), json!({"hex":hex,"sha256":digest}));
    } else {
        report.push_coded(Severity::Degraded, label, "design/reference.frozen",
            format!("Linked Insert Design needs SolidCraft 0.44 or newer; target {} keeps a frozen copy of the referenced bodies and the following supported booleans", ctx.target_version));
    }
    ctx.named_bodies.extend(bindings);
    ctx.builds = true;
    Ok(mapped)
}

/// Source properties carry strings in CADIR. The string is a versioned neutral payload.
fn payload(value: &Value) -> Result<Value, String> {
    let text = value
        .as_str()
        .filter(|text| text.len() <= 256 << 20)
        .ok_or("the neutral design reference is missing or exceeds its size limit")?;
    let value: Value = serde_json::from_str(text)
        .map_err(|error| format!("the neutral design reference cannot be read: {error}"))?;
    if value.get("schema").and_then(Value::as_str) != Some(REFERENCE_SCHEMA) {
        return Err("the neutral design reference schema is not supported yet".into());
    }
    Ok(value)
}

pub(super) fn body_bindings(feature: &Value) -> Result<Option<Vec<(String, String)>>, String> {
    let value = if let Some(value) = feature.pointer("/source_properties/historical_bodies") {
        let text = value
            .as_str()
            .filter(|text| text.len() <= 32 << 20)
            .ok_or("the historical body identities exceed their size limit")?;
        serde_json::from_str::<Value>(text).map_err(|e| e.to_string())?
    } else if let Some(value) = feature.pointer("/source_properties/design_reference") {
        payload(value)?
            .get("bodies")
            .cloned()
            .ok_or("the referenced body identities are missing")?
    } else {
        return Ok(None);
    };
    let rows = value
        .as_array()
        .filter(|rows| !rows.is_empty() && rows.len() <= 10_000)
        .ok_or("the historical body identities are missing or exceed the row limit")?;
    let mut bindings = Vec::new();
    for row in rows {
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("a historical body identity is missing")?;
        let name = row
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("a historical body name is missing")?;
        if bindings
            .iter()
            .any(|(known, taken)| known == id || taken == name)
        {
            return Err("historical body identities or names repeat".into());
        }
        bindings.push((id.to_owned(), name.to_owned()));
    }
    Ok(Some(bindings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dutchman_core::Format;

    /// The payload the Onshape reader writes for `ir`: the CADIR wire beside the arenas it omits.
    fn payload_of(ir: &cadmpeg_ir::CadIr) -> Value {
        json!({
            "schema": REFERENCE_SCHEMA,
            "ir": serde_json::to_value(ir).unwrap(),
            "omitted": ir.model.omitted_arenas_value().unwrap(),
            "step": "ISO-10303-21;",
            "name": "Studio",
            "bodies": [{"id": "body", "name": "Tool"}],
        })
    }

    fn rows<T: serde::Serialize>(rows: &[T]) -> Value {
        serde_json::to_value(rows).unwrap()
    }

    /// A payload that states features, sketches and parameters beside the wire gives back the
    /// design the history writer reads, with every omitted arena as it was.
    #[test]
    fn omitted_arenas_of_a_reference_restore_into_the_design() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures");
        let (mut features, mut sketches, mut parameters) = (0, 0, 0);
        for name in [
            "design_sketch_constraints.f3d",
            "design_copy_paste.f3d",
            "generated_unit_cube.f3d",
        ] {
            let model = dutchman_core::read(
                Format::FusionArchive,
                &root.join(name),
                &mut Report::default(),
            )
            .unwrap();
            let reference = payload_of(&model.ir);
            assert!(
                reference["ir"]["model"].get("features").is_none(),
                "{name}: the wire carries the omitted arenas itself"
            );
            let text = serde_json::to_string(&reference).unwrap();
            let read = payload(&Value::String(text)).unwrap();
            let design = neutral_design(&read).unwrap();
            let (want, got) = (&model.ir.model, &design.model);
            assert_eq!(rows(&got.features), rows(&want.features), "{name}");
            assert_eq!(rows(&got.sketches), rows(&want.sketches), "{name}");
            assert_eq!(
                rows(&got.sketch_entities),
                rows(&want.sketch_entities),
                "{name}"
            );
            assert_eq!(rows(&got.parameters), rows(&want.parameters), "{name}");
            assert_eq!(
                rows(&got.configurations),
                rows(&want.configurations),
                "{name}"
            );
            assert_eq!(
                rows(&got.assembly_joints),
                rows(&want.assembly_joints),
                "{name}"
            );
            features += want.features.len();
            sketches += want.sketches.len();
            parameters += want.parameters.len();
        }
        assert!(features > 0 && sketches > 0 && parameters > 0);
    }

    #[test]
    fn a_reference_without_its_omitted_arenas_is_refused() {
        let model = crate::testing::cube();
        let mut reference = payload_of(&model.ir);
        reference.as_object_mut().unwrap().remove("omitted");
        let error = neutral_design(&reference).unwrap_err();
        assert!(error.contains("arenas"), "{error}");
    }

    #[test]
    fn only_the_current_reference_schema_is_read() {
        let model = crate::testing::cube();
        let mut reference = payload_of(&model.ir);
        assert!(payload(&Value::String(reference.to_string())).is_ok());
        reference["schema"] = json!("dutchman.design-reference/1");
        let error = payload(&Value::String(reference.to_string())).unwrap_err();
        assert!(error.contains("not supported yet"), "{error}");
    }
}
