//! SolidCraft adapter for Dutchman: writes the neutral model as a `.solidcraft` design.
//!
//! The adapter writes the design JSON itself rather than linking SolidCraft's crates, so it
//! carries none of SolidCraft's kernel or renderer. It targets one pinned design format,
//! [`FORMAT`], which every newer SolidCraft upgrades on open.

use dutchman_core::{Error, Model, Report, Writer};

/// The SolidCraft design format this adapter writes.
pub const FORMAT: &str = "solidcraft/26";

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
        _model: &Model,
        _out: &mut dyn std::io::Write,
        _report: &mut Report,
    ) -> Result<(), Error> {
        Err(Error::Encode(format!(
            "writing {FORMAT} designs is not supported yet"
        )))
    }
}
