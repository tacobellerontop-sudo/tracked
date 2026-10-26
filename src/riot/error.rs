//! The connection layer's error type (brief §10). Every failure while
//! discovering, authenticating against or watching the Riot client maps to
//! one of these. None of them carry tokens.

use crate::error::ApiError;

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum RiotError {
    #[error("Riot client isn't running")]
    ClientNotRunning,
    #[error("couldn't read the Riot lockfile")]
    LockfileMalformed,
    #[error("you're not signed in to VALORANT")]
    SignedOut,
    #[error("region not detected (start VALORANT)")]
    RegionUnknown,
    #[error("Riot auth failed")]
    AuthFailed,
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected response: {0}")]
    Unexpected(String),
}

impl From<super::lockfile::LockfileError> for RiotError {
    fn from(e: super::lockfile::LockfileError) -> Self {
        tracing::warn!("lockfile malformed: {e}");
        RiotError::LockfileMalformed
    }
}

impl From<ApiError> for RiotError {
    fn from(e: ApiError) -> Self {
        match e {
            ApiError::NotRunning => RiotError::ClientNotRunning,
            ApiError::Lockfile(_) => RiotError::LockfileMalformed,
            ApiError::RegionUnknown => RiotError::RegionUnknown,
            ApiError::Unauthorized => RiotError::AuthFailed,
            ApiError::Network(m) => RiotError::Network(m),
            other => RiotError::Unexpected(other.to_string()),
        }
    }
}

impl From<RiotError> for ApiError {
    fn from(e: RiotError) -> Self {
        match e {
            RiotError::ClientNotRunning => ApiError::NotRunning,
            RiotError::RegionUnknown => ApiError::RegionUnknown,
            RiotError::AuthFailed => ApiError::Unauthorized,
            RiotError::Network(m) => ApiError::Network(m),
            other => ApiError::Other(other.to_string()),
        }
    }
}
