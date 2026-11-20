//! Neutral design references written using SolidCraft's Insert Design representation.
use dutchman_core::{Model, Report, Severity, Writer};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{name, Context};
use crate::{SolidCraftWriter, Verification, VersionGate};

pub(super) fn map(
    feature: &Value,
    reference: &Value,
    ctx: &mut Context,
    report: &mut Report,
) -> Result<Value, String> {
    let reference = payload(reference)?;
    let source = Model {
        ir: serde_json::from_value(
            reference
                .get("ir")
                .cloned()
                .ok_or("the referenced neutral design is missing")?,
        )
        .map_err(|e| format!("the referenced neutral design cannot be read: {e}"))?,
        source_step: Some(
            reference
                .get("step")
                .and_then(Value::as_str)
                .ok_or("the referenced geometry is missing")?
                .into(),
        ),
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
    let wire = serde_json::to_value(&source.ir).map_err(|e| e.to_string())?;
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
    if value.get("schema").and_then(Value::as_str) != Some("dutchman.design-reference/1") {
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
