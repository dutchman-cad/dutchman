//! Models for unit tests: the synthetic unit cube, with features and parameters added as
//! CADIR JSON.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use dutchman_core::{read, Format, Model, Report};
use serde_json::Value;

/// Held by every test that writes a stand-in program and runs it. A script still open for
/// writing when another test forks is inherited by that child, and running it then fails with
/// "text file busy". One lock for the whole crate keeps every writer and runner apart; a lock
/// per module did not, since tests in other modules wrote scripts at the same time.
pub(crate) fn scripts() -> std::sync::MutexGuard<'static, ()> {
    static SCRIPTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SCRIPTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The synthetic unit cube from the core fixtures.
pub(crate) fn cube() -> Model {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
    read(Format::FusionArchive, &path, &mut Report::default()).unwrap()
}

/// `model` with its feature and parameter arenas replaced.
pub(crate) fn with(model: Model, features: Value, parameters: Value) -> Model {
    let mut ir = serde_json::to_value(&model.ir).unwrap();
    ir["model"]["features"] = features;
    ir["model"]["parameters"] = parameters;
    let ir = cadmpeg_ir::CadIr::from_json(&ir.to_string()).unwrap();
    Model { ir, ..model }
}

/// A feature row with the given id key, ordinal and definition.
pub(crate) fn feature(key: u32, ordinal: u64, name: &str, definition: Value) -> Value {
    serde_json::json!({
        "id": format!("f3d:model:feature#{key}"),
        "ordinal": ordinal,
        "name": name,
        "suppressed": false,
        "definition": definition,
    })
}
