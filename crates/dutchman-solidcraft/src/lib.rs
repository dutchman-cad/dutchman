//! SolidCraft adapter for Dutchman: writes the neutral model as a `.solidcraft` design.
//!
//! The design is exactly what SolidCraft saves when it imports a STEP file: a design
//! container holding one component, one occurrence of it, and a non-parametric `import`
//! feature whose geometry is STEP text (here, cadmpeg's restatement of the model). The
//! adapter writes the container itself rather than linking SolidCraft's crates, so it carries
//! none of SolidCraft's kernel or renderer.

mod container;
mod document;
mod step;

use dutchman_core::{Error, Model, Report, Writer};

/// The SolidCraft design format this adapter writes: the first in which an Import may name a
/// non-STEP source file whose geometry it keeps as restated STEP text. Every newer
/// SolidCraft upgrades it on open.
pub const FORMAT: &str = "solidcraft/23";

/// Writes `.solidcraft` designs.
#[derive(Clone, Copy, Debug, Default)]
pub struct SolidCraftWriter;

impl SolidCraftWriter {
    /// A writer for the pinned [`FORMAT`].
    pub fn new() -> Self {
        SolidCraftWriter
    }
}

impl Writer for SolidCraftWriter {
    fn extension(&self) -> &'static str {
        "solidcraft"
    }

    fn write(
        &self,
        model: &Model,
        out: &mut dyn std::io::Write,
        report: &mut Report,
    ) -> Result<(), Error> {
        let step = step::restate(model, report)?;
        document::report_unmapped(model, report);
        let design = container::Container::new(step.as_bytes())?;
        let document = document::import_design(model, design.step_blob());
        let bytes = design.finish(&document)?;
        out.write_all(&bytes)
            .map_err(|err| Error::Encode(format!("cannot write the design: {err}")))
    }
}
