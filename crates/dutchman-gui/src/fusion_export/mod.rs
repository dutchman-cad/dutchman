//! Export from Fusion: finding the Fusion exporter add-in running in the person's Fusion, and
//! installing it with one click when it is not there. The protocol is in the exporter's
//! `docs/job-protocol.md` ("Discovery: the agent file", "Installing the add-in").

pub mod agent;
pub mod install;
pub mod panel;

/// The one place in Dutchman's public text that names the exporter's project; everywhere else it
/// is "the Fusion exporter".
pub const PROJECT_NAME: &str = "Lifeboat's Fusion exporter";
/// The exporter's project page.
pub const PROJECT_URL: &str = "https://github.com/cad-lifeboat/fusion-bulk-export";
