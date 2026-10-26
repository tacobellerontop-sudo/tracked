//! Internal errors and their mapping to short user-facing messages.

use std::fmt;

/// Errors produced by the Riot/content layers.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("VALORANT isn't running")]
    NotRunning,
    #[error("lockfile: {0}")]
    Lockfile(#[from] crate::riot::lockfile::LockfileError),
    #[error("region not detected")]
    RegionUnknown,
    #[error("refusing to call malformed host {0:?}")]
    InvalidHost(String),
    #[error("auth expired or rejected")]
    Unauthorized,
    #[error("not found")]
    NotFound,
    #[error("rate limited (retry after {0:?}s)")]
    RateLimited(Option<u64>),
    #[error("http status {0}")]
    Status(u16),
    #[error("network: {0}")]
    Network(String),
    #[error("unexpected response: {0}")]
    Decode(String),
    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        // reqwest errors can embed the full URL; our URLs never contain
        // credentials (those go in headers), so this is safe to keep.
        if e.is_decode() {
            ApiError::Decode(e.to_string())
        } else if let Some(s) = e.status() {
            ApiError::Status(s.as_u16())
        } else {
            ApiError::Network(e.without_url().to_string())
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::Decode(e.to_string())
    }
}

/// A short, friendly message safe to show in the UI. Never contains tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFacingError(pub String);

impl fmt::Display for UserFacingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&ApiError> for UserFacingError {
    fn from(e: &ApiError) -> Self {
        let msg = match e {
            ApiError::NotRunning => "VALORANT isn't running",
            ApiError::Lockfile(_) => "Couldn't read the Riot lockfile — is VALORANT running?",
            ApiError::RegionUnknown => "Riot region not detected (start VALORANT)",
            ApiError::InvalidHost(_) => "Riot region looks invalid — restart VALORANT",
            ApiError::Unauthorized => "Riot session expired — reconnecting…",
            ApiError::NotFound => "Not found",
            ApiError::RateLimited(_) => "Rate-limited by Riot, retrying…",
            ApiError::Status(s) if *s >= 500 => "Riot servers are having trouble",
            ApiError::Status(_) => "Riot rejected the request",
            ApiError::Network(_) => "Couldn't reach Riot servers",
            ApiError::Decode(_) => "Riot returned data we couldn't understand",
            ApiError::Other(m) => return UserFacingError(m.clone()),
        };
        UserFacingError(msg.to_string())
    }
}

impl From<ApiError> for UserFacingError {
    fn from(e: ApiError) -> Self {
        (&e).into()
    }
}
