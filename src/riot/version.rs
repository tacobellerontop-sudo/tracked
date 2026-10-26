//! `X-Riot-ClientVersion`: where it comes from and when to refresh it.

use std::time::Duration;

use super::error::RiotError;

/// Refetch the version at least this often while running.
pub const MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// The current client version: `TRACKED_CLIENT_VERSION` (development
/// override), then valorant-api.com, then the "CI server version" line in
/// VALORANT's own log file.
pub async fn fetch(http: &reqwest::Client) -> Result<String, RiotError> {
    if let Ok(v) = std::env::var("TRACKED_CLIENT_VERSION") {
        return Ok(v);
    }
    match crate::content::valorant_api::client_version(http).await {
        Ok(v) => Ok(v),
        Err(e) => match from_log().await {
            Some(v) => Ok(v),
            None => Err(RiotError::Network(format!("couldn't fetch the client version: {e}"))),
        },
    }
}

/// Read `%LOCALAPPDATA%\VALORANT\Saved\Logs\ShooterGame.log`. This is an
/// ordinary text file the game writes; we only read it.
pub async fn from_log() -> Option<String> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let path = std::path::PathBuf::from(local)
        .join("VALORANT")
        .join("Saved")
        .join("Logs")
        .join("ShooterGame.log");
    let text = tokio::fs::read_to_string(path).await.ok()?;
    parse_ci_version(&text)
}

/// `CI server version: release-09.05-shipping-21-2843553` -> the version.
pub fn parse_ci_version(log: &str) -> Option<String> {
    log.lines()
        .find_map(|l| l.split_once("CI server version:").map(|(_, v)| v.trim().to_string()))
        .filter(|v| !v.is_empty())
}

/// Does a 400 body look like a client-version problem rather than an auth
/// one? (Expired tokens come back as 400 `BAD_CLAIMS`.)
pub fn looks_like_version_error(body: &str) -> bool {
    let b = body.to_ascii_uppercase();
    !b.contains("BAD_CLAIMS") && (b.contains("VERSION") || b.contains("CLIENT_PLATFORM") || b.contains("UPGRADE"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_error_detection() {
        assert!(looks_like_version_error(
            r#"{"httpStatus":400,"errorCode":"INVALID_CLIENT_VERSION"}"#
        ));
        assert!(!looks_like_version_error(
            r#"{"httpStatus":400,"errorCode":"BAD_CLAIMS"}"#
        ));
        assert!(!looks_like_version_error("{}"));
    }
}
