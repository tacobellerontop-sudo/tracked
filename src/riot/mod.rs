//! The Riot client connection layer, plus the Riot data clients built on it.
//!
//! Everything here talks HTTP/WebSocket only: the Riot Client's local server
//! on 127.0.0.1 and Riot's public `*.pvp.net` endpoints, using tokens the
//! local server issued. No process access of any kind: no memory reading,
//! injection, hooking, drivers or input.
//!
//! Connection layer (the module's public API):
//! * [`connect`] / [`connect_observed`] → an authenticated [`RiotSession`]
//! * [`watch_state`] / [`watch`] → live [`GameState`]
//! * [`refresh_tokens`] → swap in fresh tokens

pub mod account;
pub mod auth;
pub mod chat;
pub mod error;
pub mod glz;
pub mod local;
pub mod lockfile;
pub mod match_stats;
pub mod models;
pub mod names;
pub mod pd;
pub mod presence;
pub mod region;
pub mod session;
pub mod state;
pub mod tls;
pub mod version;

use std::time::Duration;

pub use error::RiotError;
pub use region::{Hosts, Region, Shard};
pub use session::{connect, connect_observed, refresh_tokens, RegionSource, RiotSession};
pub use state::{watch, watch_state, GameState, WatchEvent};
pub use version::parse_ci_version;

/// Kept so older call sites read naturally.
pub type Session = RiotSession;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiotId {
    pub game_name: String,
    pub tag_line: String,
}

impl std::fmt::Display for RiotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.game_name, self.tag_line)
    }
}

/// Shared HTTP client for public internet calls (Riot PD/GLZ, valorant-api).
/// Uses normal certificate verification; never used for 127.0.0.1.
pub fn public_http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("ascend-lite/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(15))
        .build()
        .expect("failed to build HTTP client")
}
