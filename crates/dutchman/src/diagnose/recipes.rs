//! Release data for rebuilding failures without copying a design's dimensions or names.
use std::collections::BTreeMap;

use serde::Deserialize;

use super::signature::Signature;

#[derive(Deserialize)]
struct Catalog {
    schema: String,
    recipes: Vec<Template>,
    programs: Vec<Program>,
    generic: Vec<String>,
}

#[derive(Deserialize)]
struct Template {
    class: String,
    steps: Vec<String>,
}

#[derive(Deserialize)]
struct Program {
    name: String,
    new: String,
    save: String,
    operations: BTreeMap<String, String>,
    hints: BTreeMap<String, String>,
}

/// The recipe class, independent of private design information.
pub fn class(signature: &Signature) -> &str {
    signature
        .feature
        .as_ref()
        .map_or("generic", |f| f.kind.as_str())
}

/// Numbered instructions for each supported authoring program. Dimensions are invented,
/// round starting sizes, never measurements of the source. A recipe is only a candidate:
/// `same_failure` must confirm it.
pub fn render(signature: &Signature) -> String {
    let catalog =
        serde_json::from_str::<Catalog>(include_str!("../../data/rebuild-recipes-v1.json"))
            .ok()
            .filter(|catalog| catalog.schema == "dutchman.recipes/1");
    let Some(catalog) = catalog else {
        return format!("Rebuild a small new file showing: {}. Use round sizes, then check it with --same-failure.\n", signature.describe());
    };
    let steps = catalog
        .recipes
        .iter()
        .find(|r| r.class == class(signature))
        .map_or(&catalog.generic, |r| &r.steps);
    let mut text = format!("Failure signature {}: it {}.\n\nThese are starting sizes; the check must confirm the same failure.\n\n", signature.short(), signature.describe());
    for program in &catalog.programs {
        text.push_str(&format!("### {}\n\n1. {}\n", program.name, program.new));
        for (index, step) in steps.iter().enumerate() {
            let operation = program
                .operations
                .get(class(signature))
                .map_or(class(signature), String::as_str);
            text.push_str(&format!(
                "{}. {}\n",
                index + 2,
                step.replace("{operation}", operation)
            ));
        }
        if let Some(hint) = program.hints.get(class(signature)) {
            text.push_str(&format!("   {hint}\n"));
        }
        text.push_str(&format!(
            "{}. {} Use fresh names and keep the original private.\n\n",
            steps.len() + 2,
            program.save
        ));
    }
    text.push_str("1. Run `dutchman diagnose --same-failure <small file> --out <this folder>`. If it converts fine or fails differently, revisit the recorded options and profile above and try again.\n2. Run `dutchman diagnose --make-report <small file> --out <this folder>`. Review the ZIP before sharing: it contains the small file's geometry. Nothing is sent.\n\n");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnose::signature::{ErrorCode, FeatureShape};

    #[test]
    fn catalog_and_fallback_cover_every_program_and_preserve_signature_details() {
        let catalog: Catalog =
            serde_json::from_str(include_str!("../../data/rebuild-recipes-v1.json")).unwrap();
        assert_eq!(catalog.schema, "dutchman.recipes/1");
        for kind in [
            "extrude",
            "revolve",
            "fillet",
            "chamfer",
            "hole",
            "future_operation",
        ] {
            let signature = Signature::new(
                ErrorCode::default(),
                Some(FeatureShape {
                    kind: kind.into(),
                    options: [("extent".into(), "two_sided".into())].into(),
                    ..Default::default()
                }),
            );
            let text = render(&signature);
            for name in ["Fusion", "Inventor", "SolidWorks", "FreeCAD"] {
                assert!(text.contains(name));
            }
            assert!(text.contains("two sided"));
            assert!(text.contains("--same-failure"));
            assert!(!text.contains("{operation}"));
        }
    }
}
