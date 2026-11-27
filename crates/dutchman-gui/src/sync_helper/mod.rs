//! Dutchman Sync: the small background helper that keeps converted designs up to date with
//! their source files. It is the `dutchman-sync` program; the code lives here so it can be
//! tested.
//!
//! - [`engine`] runs `dutchman::sync::watch` for every folder the shared state turns on.
//! - [`controller`] is the loop behind the tray menu.
//! - [`tray`] is the icon and menu; [`notify`] the desktop notifications.
//! - [`instance`] keeps it to one copy; [`login`] writes the start-at-login entry.
//! - [`window`] is the first-run window; [`app`] puts it all together.
//!
//! What the app, the tray and this program agree on is the shared state file,
//! [`dutchman::sync_state`].

pub mod app;
pub mod controller;
pub mod engine;
pub mod icon;
pub mod instance;
pub mod login;
pub mod notify;
pub mod tray;
pub mod window;
