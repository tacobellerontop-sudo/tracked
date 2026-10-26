//! Ascend-Lite: an unofficial, read-only VALORANT companion.
//!
//! The crate is split into a UI half (`app`, `ui`) that runs on the egui
//! thread and a worker half (`worker`, `riot`, `content`) that runs on a
//! tokio runtime. The two halves talk only through the [`ipc`] channels.

pub mod app;
pub mod ipc;
pub mod ui;
pub mod worker;
