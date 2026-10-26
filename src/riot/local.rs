//! Client for the Riot Client's local REST API on `https://127.0.0.1:<port>`
//! (brief §3).
//!
//! The local server uses HTTP basic auth (`riot:<lockfile password>`) and a
//! self-signed certificate. This client can only ever reach 127.0.0.1: its
//! base URL is fixed, and its default TLS backend (rustls with
//! [`tls::LoopbackOnlyVerifier`]) refuses any non-loopback target. On Windows,
//! if rustls can't complete the handshake with the local server, it falls back
//! once to the system TLS stack (schannel) with certificate checks off. That is
//! the classic `danger_accept_invalid_certs` client, still only pointed at
//! 127.0.0.1.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;

use base64::Engine;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use super::lockfile::Lockfile;
use super::tls;
use crate::error::ApiError;
use crate::secret::Secret;

const TIMEOUT: Duration = Duration::from_secs(5);
/// Retries for network errors right after the client launches.
const RETRIES: u32 = 2;
const RETRY_DELAY: Duration = Duration::from_millis(400);

/// Credentials for the local server.
#[derive(Debug, Clone)]
pub struct LocalAuth {
    /// `https://127.0.0.1:<port>`
    pub base: String,
    pub port: u16,
    pub pid: u32,
    /// `base64("riot:<password>")`, the HTTP basic credential. Secret.
    pub basic: Secret<String>,
    /// The raw lockfile password (the websocket needs it too). Secret.
    pub password: Secret<String>,
}

impl LocalAuth {
    pub fn from_lockfile(lock: &Lockfile) -> Self {
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("riot:{}", lock.password.expose()));
        Self {
            base: format!("https://127.0.0.1:{}", lock.port),
            port: lock.port,
            pid: lock.pid,
            basic: Secret::new(basic),
            password: lock.password.clone(),
        }
    }

    /// `Basic <base64>` as a header value marked sensitive.
    pub fn header(&self) -> reqwest::header::HeaderValue {
        let mut v = reqwest::header::HeaderValue::from_str(&format!("Basic {}", self.basic.expose()))
            .expect("base64 is a valid header value");
        v.set_sensitive(true);
        v
    }
}

/// Which TLS implementation talks to the local server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsBackend {
    /// rustls + loopback-only verifier (default, all platforms).
    Rustls,
    /// Windows schannel with certificate checks off (fallback).
    System,
}

pub struct LocalClient {
    auth: LocalAuth,
    http: RwLock<(TlsBackend, reqwest::Client)>,
}

impl std::fmt::Debug for LocalClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalClient")
            .field("base", &self.auth.base)
            .field("backend", &self.backend())
            .finish()
    }
}

/// `GET /entitlements/v1/token`
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementsResponse {
    pub access_token: Secret<String>,
    /// Entitlements JWT.
    pub token: Secret<String>,
    /// The player's PUUID.
    pub subject: String,
}

/// `GET /chat/v1/session`
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ChatSession {
    pub puuid: String,
    pub game_name: String,
    pub game_tag: String,
    pub region: String,
    pub loaded: bool,
}

/// One entry of `GET /product-session/v1/external-sessions`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExternalSession {
    pub product_id: String,
    pub launch_configuration: LaunchConfiguration,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct LaunchConfiguration {
    pub arguments: Vec<String>,
}

/// `GET /riotclient/region-locale`
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RegionLocale {
    pub region: String,
    pub web_region: String,
}

/// `GET /chat/v4/presences`
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Presences {
    pub presences: Vec<super::presence::RawPresence>,
}

/// `GET /chat/v4/friends`
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Friends {
    pub friends: Vec<Friend>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Friend {
    pub puuid: String,
    pub game_name: String,
    pub game_tag: String,
}

fn rustls_client() -> Result<reqwest::Client, ApiError> {
    reqwest::Client::builder()
        .use_preconfigured_tls((*tls::local_client_config()).clone())
        .no_proxy()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| ApiError::Other(format!("couldn't build local client: {e}")))
}

#[cfg(windows)]
fn system_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .use_native_tls()
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .timeout(TIMEOUT)
        .build()
        .ok()
}

#[cfg(not(windows))]
fn system_client() -> Option<reqwest::Client> {
    None
}

/// Does this network error look like a TLS handshake problem (as opposed
/// to "connection refused" / timeouts)?
fn is_tls_error(detail: &str) -> bool {
    let d = detail.to_ascii_lowercase();
    ["tls", "handshake", "certificate", "alert", "peer"]
        .iter()
        .any(|k| d.contains(k))
}

impl LocalClient {
    pub fn new(lock: &Lockfile) -> Result<Self, ApiError> {
        Ok(Self {
            auth: LocalAuth::from_lockfile(lock),
            http: RwLock::new((TlsBackend::Rustls, rustls_client()?)),
        })
    }

    pub fn auth(&self) -> &LocalAuth {
        &self.auth
    }

    pub fn port(&self) -> u16 {
        self.auth.port
    }

    pub fn pid(&self) -> u32 {
        self.auth.pid
    }

    pub fn password(&self) -> &Secret<String> {
        &self.auth.password
    }

    pub fn backend(&self) -> TlsBackend {
        self.http.read().unwrap().0
    }

    /// GET a local endpoint, retrying briefly on network errors (the client
    /// refuses connections for a moment after it starts).
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        debug_assert!(path.starts_with('/'));
        let mut attempt = 0;
        loop {
            match self.get_once(path).await {
                Err(ApiError::Network(detail)) => {
                    if self.backend() == TlsBackend::Rustls && is_tls_error(&detail) {
                        if let Some(system) = system_client() {
                            tracing::warn!(
                                "local TLS handshake failed with rustls ({detail}); using the system TLS stack"
                            );
                            *self.http.write().unwrap() = (TlsBackend::System, system);
                            continue;
                        }
                    }
                    if attempt >= RETRIES {
                        return Err(ApiError::Network(detail));
                    }
                    attempt += 1;
                    tokio::time::sleep(RETRY_DELAY * attempt).await;
                }
                other => return other,
            }
        }
    }

    async fn get_once<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let http = self.http.read().unwrap().1.clone();
        let resp = http
            .get(format!("{}{}", self.auth.base, path))
            .header(reqwest::header::AUTHORIZATION, self.auth.header())
            .send()
            .await
            .map_err(|e| ApiError::Network(crate::error::chain(&e.without_url())))?;
        match resp.status().as_u16() {
            200..=299 => Ok(resp.json().await?),
            401 | 403 => Err(ApiError::Unauthorized),
            404 => Err(ApiError::NotFound),
            s => Err(ApiError::Status(s)),
        }
    }

    /// The TLS connector the presence websocket should use: same backend as
    /// the REST client.
    pub fn ws_connector(&self) -> tokio_tungstenite::Connector {
        match self.backend() {
            TlsBackend::Rustls => tokio_tungstenite::Connector::Rustls(tls::local_client_config()),
            TlsBackend::System => system_ws_connector(),
        }
    }

    pub async fn entitlements(&self) -> Result<EntitlementsResponse, ApiError> {
        self.get("/entitlements/v1/token").await
    }

    pub async fn chat_session(&self) -> Result<ChatSession, ApiError> {
        self.get("/chat/v1/session").await
    }

    pub async fn external_sessions(&self) -> Result<HashMap<String, ExternalSession>, ApiError> {
        self.get("/product-session/v1/external-sessions").await
    }

    pub async fn region_locale(&self) -> Result<RegionLocale, ApiError> {
        self.get("/riotclient/region-locale").await
    }

    pub async fn presences(&self) -> Result<Presences, ApiError> {
        self.get("/chat/v4/presences").await
    }

    pub async fn friends(&self) -> Result<Friends, ApiError> {
        self.get("/chat/v4/friends").await
    }
}

#[cfg(windows)]
fn system_ws_connector() -> tokio_tungstenite::Connector {
    match native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
    {
        Ok(c) => tokio_tungstenite::Connector::NativeTls(c),
        Err(_) => tokio_tungstenite::Connector::Rustls(tls::local_client_config()),
    }
}

#[cfg(not(windows))]
fn system_ws_connector() -> tokio_tungstenite::Connector {
    tokio_tungstenite::Connector::Rustls(tls::local_client_config())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_is_base64_of_riot_colon_password() {
        let lock = super::super::lockfile::parse("Riot Client:1:2:s3cr3t-pw:https").unwrap();
        let a = LocalAuth::from_lockfile(&lock);
        assert_eq!(
            a.basic.expose(),
            &base64::engine::general_purpose::STANDARD.encode("riot:s3cr3t-pw")
        );
        assert_eq!(a.base, "https://127.0.0.1:2");
        assert!(a.header().is_sensitive());
        assert!(!format!("{a:?}").contains("s3cr3t"));
    }

    #[test]
    fn tls_error_detection() {
        assert!(is_tls_error(
            "error sending request: client error (Connect): received fatal alert: HandshakeFailure"
        ));
        assert!(is_tls_error("invalid peer certificate: BadEncoding"));
        assert!(!is_tls_error("tcp connect error: Connection refused (os error 111)"));
    }
}
