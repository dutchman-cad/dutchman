//! The model's geometry restated as STEP text, the form a SolidCraft Import keeps.

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_registry::{build_encoder, Format};
use dutchman_core::{Error, Model, Report};

/// The STEP schema written: AP214, which SolidCraft writes and reads natively.
const SCHEMA: &str = "ap214";

/// The name cadmpeg's STEP writer gives a product it has no name for.
const PLACEHOLDER: &str = "'cadmpeg_model'";

/// The largest STEP text SolidCraft's reader accepts.
const MAX_STEP_BYTES: usize = 512 << 20;

/// Restates the model as STEP AP214 text, recording what the restatement drops.
pub(crate) fn restate(model: &Model, report: &mut Report) -> Result<String, Error> {
    let input = EncodeInput::new(&model.ir, model.fidelity.as_ref());
    let plan = build_encoder(Format::Step)
        .plan(input, TargetRequest::Explicit(SCHEMA))
        .map_err(|err| Error::Encode(format!("cannot restate the model as STEP: {err}")))?;
    let losses = &plan.report().losses;
    if losses
        .iter()
        .any(|loss| loss.code.to_string() == "step/export.no-exportable-solids")
    {
        return Err(Error::Encode(
            "the model has no solids or surfaces SolidCraft can import".to_owned(),
        ));
    }
    report.extend_from_cadmpeg(losses);

    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .map_err(|err| Error::Encode(format!("cannot write STEP: {err}")))?;
    if bytes.len() > MAX_STEP_BYTES {
        return Err(Error::Encode(format!(
            "the STEP restatement is {} MB, over SolidCraft's {} MB import limit",
            bytes.len() >> 20,
            MAX_STEP_BYTES >> 20
        )));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::Encode("cadmpeg wrote STEP that is not UTF-8".to_owned()))?;
    Ok(name_unnamed_product(text, &model.name))
}

/// Names the product cadmpeg left unnamed after the design, as SolidCraft does for files it
/// restates through cadmpeg. The name is reduced to plain STEP string characters.
fn name_unnamed_product(step: String, design: &str) -> String {
    let name: String = design
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        .collect();
    if name.trim().is_empty() || !step.contains(PLACEHOLDER) {
        step
    } else {
        step.replace(PLACEHOLDER, &format!("'{name}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_placeholder_product_after_the_design() {
        let step =
            "FILE_NAME('cadmpeg_model');\n#1=PRODUCT('cadmpeg_model','cadmpeg_model','',(#2));";
        assert_eq!(
            name_unnamed_product(step.to_owned(), "Bracket v2"),
            "FILE_NAME('Bracket v2');\n#1=PRODUCT('Bracket v2','Bracket v2','',(#2));"
        );
    }

    #[test]
    fn keeps_the_placeholder_when_the_name_has_no_plain_characters() {
        let step = "PRODUCT('cadmpeg_model')";
        assert_eq!(name_unnamed_product(step.to_owned(), "'''"), step);
    }

    #[test]
    fn strips_quotes_from_the_design_name() {
        let step = "PRODUCT('cadmpeg_model')";
        assert_eq!(
            name_unnamed_product(step.to_owned(), "it's"),
            "PRODUCT('its')"
        );
    }
}
