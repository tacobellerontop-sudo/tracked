//! Client for the Riot Client's local REST API on `https://127.0.0.1:<port>`.

use std::collections::HashMap;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;

use super::lockfile::Lockfile;
use super::tls;
use crate::error::ApiError;
use crate::secret::Secret;

/// REST client scoped to the local Riot API. Its TLS config only trusts
/// loopback addresses (see [`tls`]), so it cannot be misused for internet calls.
pub struct LocalClient {
    base: String,
    port: u16,
    pid: u32,
    password: Secret<String>,
    http: reqwest::Client,
}

impl std::fmt::Debug for LocalClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalClient")
            .field("base", &self.base)
            .field("password", &self.password)
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

impl LocalClient {
    pub fn new(lock: &Lockfile) -> Result<Self, ApiError> {
        let http = reqwest::Client::builder()
            .use_preconfigured_tls((*tls::local_client_config()).clone())
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ApiError::Other(format!("couldn't build local client: {e}")))?;
        Ok(Self {
            base: format!("https://127.0.0.1:{}", lock.port),
            port: lock.port,
            pid: lock.pid,
            password: lock.password.clone(),
            http,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn password(&self) -> &Secret<String> {
        &self.password
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        debug_assert!(path.starts_with('/'));
        let resp = self
            .http
            .get(format!("{}{}", self.base, path))
            .basic_auth("riot", Some(self.password.expose()))
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
