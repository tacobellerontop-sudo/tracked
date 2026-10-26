//! Everything that talks to Riot: the local client API and the PD/GLZ
//! endpoints, using tokens the local API issued. No process access of any kind.

pub mod account;
pub mod auth;
pub mod glz;
pub mod local;
pub mod lockfile;
pub mod match_stats;
pub mod models;
pub mod names;
pub mod pd;
pub mod presence;
pub mod region;
pub mod tls;

use std::time::Duration;

use reqwest::header::HeaderMap;
use serde::de::DeserializeOwned;
use tokio::sync::{RwLock, Semaphore};

use crate::error::ApiError;
use auth::RiotAuth;
use local::LocalClient;
use lockfile::Lockfile;
use region::Region;

/// Max concurrent requests to Riot's public endpoints.
const MAX_CONCURRENT: usize = 4;
/// Longest we'll honour a `Retry-After` before giving up.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(10);

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

/// An authenticated connection to Riot for the signed-in player.
#[derive(Debug)]
pub struct Session {
    pub local: LocalClient,
    pub region: Region,
    pub puuid: String,
    pub riot_id: Option<RiotId>,
    http: reqwest::Client,
    auth: RwLock<RiotAuth>,
    limiter: Semaphore,
}

impl Session {
    /// Lockfile -> local API -> tokens -> region -> identity.
    pub async fn connect(lock: &Lockfile, http: reqwest::Client, client_version: String) -> Result<Self, ApiError> {
        let local = LocalClient::new(lock)?;
        let ent = local.entitlements().await?;
        let region = detect_region(&local).await?;
        let riot_id = local
            .chat_session()
            .await
            .ok()
            .filter(|c| !c.game_name.is_empty())
            .map(|c| RiotId {
                game_name: c.game_name,
                tag_line: c.game_tag,
            });
        Self::from_parts(local, ent, region, riot_id, http, client_version)
    }

    /// Assemble a session from steps the caller already ran (the startup
    /// checklist runs them one at a time so it can report each).
    pub fn from_parts(
        local: LocalClient,
        ent: local::EntitlementsResponse,
        region: Region,
        riot_id: Option<RiotId>,
        http: reqwest::Client,
        client_version: String,
    ) -> Result<Self, ApiError> {
        // Under the test-only fake backend, PD/GLZ live on loopback with a
        // self-signed cert, so use the loopback-only TLS client for them.
        let http = if region.fake_base.is_some() {
            local_http()?
        } else {
            http
        };
        tracing::info!(region = %region.region, shard = %region.shard, "riot session established");
        Ok(Self {
            region,
            puuid: ent.subject.clone(),
            riot_id,
            auth: RwLock::new(RiotAuth {
                access_token: ent.access_token,
                entitlements_jwt: ent.token,
                puuid: ent.subject,
                client_version,
            }),
            local,
            http,
            limiter: Semaphore::new(MAX_CONCURRENT),
        })
    }

    pub fn pd(&self) -> Result<String, ApiError> {
        self.region.pd_base()
    }

    pub fn glz(&self) -> Result<String, ApiError> {
        self.region.glz_base()
    }

    /// Re-fetch tokens from the local API (they expire roughly hourly).
    async fn refresh_auth(&self) -> Result<(), ApiError> {
        let ent = self.local.entitlements().await?;
        let mut auth = self.auth.write().await;
        auth.access_token = ent.access_token;
        auth.entitlements_jwt = ent.token;
        tracing::info!("riot tokens refreshed");
        Ok(())
    }

    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, ApiError> {
        self.send(url, |http, h| http.get(url).headers(h)).await
    }

    pub async fn put<B: serde::Serialize, T: DeserializeOwned>(&self, url: &str, body: &B) -> Result<T, ApiError> {
        self.send(url, |http, h| http.put(url).headers(h).json(body)).await
    }

    async fn send<T, F>(&self, url: &str, build: F) -> Result<T, ApiError>
    where
        T: DeserializeOwned,
        F: Fn(&reqwest::Client, HeaderMap) -> reqwest::RequestBuilder,
    {
        // Auth headers only ever go to Riot's own API hosts.
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let fake = self.region.fake_base.is_some() && host == "127.0.0.1";
        if !region::is_riot_api_host(&host) && !fake {
            return Err(ApiError::InvalidHost(host));
        }

        let mut refreshed = false;
        let mut retries = 0;
        loop {
            let headers = self.auth.read().await.headers()?;
            let resp = {
                let _permit = self
                    .limiter
                    .acquire()
                    .await
                    .map_err(|_| ApiError::Other("shutting down".into()))?;
                build(&self.http, headers).send().await?
            };
            let status = resp.status().as_u16();
            match status {
                200..=299 => return Ok(resp.json().await?),
                400 | 401 if !refreshed => {
                    // Riot answers expired tokens with 400 BAD_CLAIMS or 401.
                    let body = resp.text().await.unwrap_or_default();
                    if status == 401 || body.contains("BAD_CLAIMS") || body.contains("token") {
                        refreshed = true;
                        self.refresh_auth().await?;
                        continue;
                    }
                    return Err(ApiError::Status(status));
                }
                401 => return Err(ApiError::Unauthorized),
                404 => return Err(ApiError::NotFound),
                429 => {
                    let after = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok());
                    let wait = Duration::from_secs(after.unwrap_or(2));
                    if retries >= 2 || wait > MAX_RETRY_AFTER {
                        return Err(ApiError::RateLimited(after));
                    }
                    retries += 1;
                    tracing::warn!(?wait, "rate limited by riot, backing off");
                    tokio::time::sleep(wait).await;
                }
                s => return Err(ApiError::Status(s)),
            }
        }
    }
}

fn local_http() -> Result<reqwest::Client, ApiError> {
    reqwest::Client::builder()
        .use_preconfigured_tls((*tls::local_client_config()).clone())
        .no_proxy()
        .build()
        .map_err(|e| ApiError::Other(e.to_string()))
}

/// Where a detected region came from (shown on the startup screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionSource {
    LaunchArgs,
    ClientRegion,
    ChatSession,
}

/// Region from VALORANT's launch args (most reliable), then the Riot
/// Client's own region, then the chat session's region.
pub async fn detect_region(local: &LocalClient) -> Result<Region, ApiError> {
    detect_region_with_source(local).await.map(|(r, _)| r)
}

pub async fn detect_region_with_source(local: &LocalClient) -> Result<(Region, RegionSource), ApiError> {
    if let Ok(sessions) = local.external_sessions().await {
        for s in sessions.values().filter(|s| s.product_id == "valorant") {
            let args = &s.launch_configuration.arguments;
            let hint = region::region_from_launch_args(args).or_else(|| region::shard_from_launch_args(args));
            if let Some(region) = hint.and_then(|h| region::region_from_hint(&h)) {
                return Ok((Region::new(&region)?, RegionSource::LaunchArgs));
            }
        }
    }
    if let Ok(rl) = local.region_locale().await {
        for hint in [&rl.web_region, &rl.region] {
            if let Some(region) = region::region_from_hint(hint) {
                return Ok((Region::new(&region)?, RegionSource::ClientRegion));
            }
        }
    }
    if let Ok(chat) = local.chat_session().await {
        if let Some(region) = region::region_from_hint(&chat.region) {
            return Ok((Region::new(&region)?, RegionSource::ChatSession));
        }
    }
    Err(ApiError::RegionUnknown)
}

/// Read the client version from `%LOCALAPPDATA%\VALORANT\Saved\Logs\ShooterGame.log`.
/// This is an ordinary text file the game writes; we only read it.
pub async fn client_version_from_log() -> Option<String> {
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

/// Shared HTTP client for public internet calls (Riot PD/GLZ, valorant-api).
/// Uses normal certificate verification.
pub fn public_http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("ascend-lite/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(15))
        .build()
        .expect("failed to build HTTP client")
}
