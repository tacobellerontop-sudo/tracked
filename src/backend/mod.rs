//! Optional Tracked-style backend (Phase 3): social features, presence,
//! lobbies, DMs, version gate. Entirely off unless a backend URL is set.
//!
//! The backend never receives Riot credentials: this module has no access to
//! the Riot [`Session`](crate::riot::Session) at all. It authenticates with
//! its own bearer JWT, which is kept in memory only.

pub mod client;
pub mod models;
pub mod social_ws;

pub use client::BackendClient;
