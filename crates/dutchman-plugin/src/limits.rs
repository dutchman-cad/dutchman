//! Shared stream and result budgets.
pub const CHUNK_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024 * 1024;
/// The host takes a loss report under 1 MiB.
pub const MAX_REPORT_BYTES: usize = 1024 * 1024 - 1;
