//! Versioned, target-neutral replay plans and deterministic script packages.
#![deny(missing_docs)]
/// Plan construction from mapped history.
pub mod build;
/// Script emitter interface and Writer adapter.
pub mod emitter;
/// Deterministic ZIP packages and plan digests.
pub mod package;
/// Versioned replay plan records.
pub mod plan;
/// Supported history prefixes and option checks.
pub mod prefix;
/// Bounded quantity evaluation from mapper expressions.
pub mod quantity;
/// Validated runtime result records.
pub mod result;
/// Lossless conversion between mapper rows and plan steps.
pub mod row;
/// Bounded TSV encoding for script calls.
pub mod tsv;

pub use emitter::{Emitter, Lowered, ScriptWriter};
pub use plan::*;
pub use result::ScriptResult;

#[cfg(test)]
mod tests;

// Exercise the original prefix implementation directly without changing its crate.
#[cfg(test)]
#[path = "../../dutchman-freecad/src/prefix.rs"]
mod freecad_prefix;
#[cfg(test)]
#[path = "../../dutchman-freecad/src/generate.rs"]
mod generate;
