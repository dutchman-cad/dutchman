//! Shared HTTP guards and bounded zip extraction for local services.

pub mod hardening;
pub mod upload;

pub use hardening::prepare_data_dir;
