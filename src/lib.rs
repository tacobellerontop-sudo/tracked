//! Ascend-Lite: an unofficial, read-only VALORANT companion.
//!
//! The crate is split into a UI half (`app`, `ui`) that runs on the egui
//! thread and a worker half (`worker`, `riot`, `content`) that runs on a
//! tokio runtime. The two halves talk only through the [`ipc`] channels.
//!
//! This crate never reads or writes game memory, never injects into or hooks
//! any process, and never automates input. All data comes from the Riot
//! Client's local API and Riot's web endpoints using tokens it issued.

pub mod app;
pub mod backend;
pub mod config;
pub mod content;
pub mod crosshair;
pub mod diagnostics;
pub mod error;
pub mod ipc;
pub mod logging;
pub mod riot;
pub mod secret;
pub mod ui;
pub mod worker;
