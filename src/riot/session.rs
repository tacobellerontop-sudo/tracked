//! `RiotSession`: discover, authenticate, and call Riot (brief §2-§6, §8).

use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use tokio::sync::{Mutex, RwLock, Semaphore};

use super::auth::PublicAuth;
use super::error::RiotError;
use super::local::{ChatSession, LocalAuth, LocalClient};
use super::region::{self, Hosts, Region, Shard};
use super::{lockfile, version, RiotId};
use crate::diagnostics::{Step, StepState};
use crate::error::ApiError;

/// Max concurrent requests to Riot's public endpoints.
const MAX_CONCURRENT: usize = 4;
/// Longest we'll honour a `Retry-After` before giving up.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(10);

/// Everything needed to call Riot's local and public APIs.
///
/// Shared between tasks behind an `Arc`, so the parts that change while
/// running (tokens, client version) sit behind locks; see [`Self::public`].
#[derive(Debug)]
pub struct RiotSession {
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
    pub region: Region,
    pub shard: Shard,
    pub hosts: Hosts,
    /// Local server client (holds [`LocalAuth`]).
    pub local: LocalClient,
    public: RwLock<PublicAuth>,
    version_checked: Mutex<Instant>,
    http: reqwest::Client,
    limiter: Semaphore,
}

/// How a region was determined (shown on the startup screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionSource {
    LaunchArgs,
    ClientRegion,
    ChatSession,
    TokenClaim,
}

impl RegionSource {
    pub fn describe(self) -> &'static str {
        match self {
            RegionSource::LaunchArgs => "from VALORANT",
            RegionSource::ClientRegion => "from the Riot Client",
            RegionSource::ChatSession => "from your chat session",
            RegionSource::TokenClaim => "from your sign-in token",
        }
    }
}

/// Discover the running client and build a fully authenticated session.
pub async fn connect() -> Result<RiotSession, RiotError> {
    connect_observed(&super::public_http(), &mut |_, _| {}).await
}

/// [`connect`], reporting each step to `observe` (for the startup checklist).
pub async fn connect_observed(
    http: &reqwest::Client,
    observe: &mut dyn FnMut(Step, StepState),
) -> Result<RiotSession, RiotError> {
    // §2 lockfile.
    observe(Step::Lockfile, StepState::Running);
    let lock = match lockfile::read().await {
        Ok(l) => l,
        Err(e) => {
            let state = match &e {
                RiotError::ClientNotRunning => StepState::failed(
                    format!(
                        "The Riot Client isn't running (no lockfile at {})",
                        lockfile::default_path()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    ),
                    "Start the Riot Client and launch VALORANT. This updates automatically.",
                ),
                other => StepState::failed(other.to_string(), "Restart the Riot Client."),
            };
            observe(Step::Lockfile, state);
            return Err(e);
        }
    };
    observe(
        Step::Lockfile,
        StepState::Ok(format!(
            "Found the Riot Client on port {} (pid {})",
            lock.port, lock.pid
        )),
    );

    // §3-4 local API: signed in? then tokens.
    observe(Step::LocalApi, StepState::Running);
    let local = LocalClient::new(&lock).map_err(RiotError::from)?;
    let chat = match local.chat_session().await {
        Ok(c) if c.loaded && !c.puuid.is_empty() => c,
        Ok(_) | Err(ApiError::NotFound) => {
            observe(
                Step::LocalApi,
                StepState::failed(
                    "The Riot Client is open but you're not signed in yet",
                    "Sign in to the Riot Client.",
                ),
            );
            return Err(RiotError::SignedOut);
        }
        Err(e) => {
            observe(Step::LocalApi, local_failure(&e));
            return Err(e.into());
        }
    };
    let ent = match local.entitlements().await {
        Ok(e) => e,
        Err(ApiError::NotFound) => {
            observe(
                Step::LocalApi,
                StepState::failed("Signed in, but no game tokens yet", "Launch VALORANT."),
            );
            return Err(RiotError::SignedOut);
        }
        Err(e) => {
            observe(Step::LocalApi, local_failure(&e));
            return Err(e.into());
        }
    };
    if !ent.subject.eq_ignore_ascii_case(&chat.puuid) {
        let msg = "The chat session and the tokens belong to different accounts";
        observe(Step::LocalApi, StepState::failed(msg, "Restart the Riot Client."));
        return Err(RiotError::Unexpected(msg.into()));
    }
    let short = chat.puuid.get(..8).unwrap_or(&chat.puuid).to_string();
    observe(
        Step::LocalApi,
        StepState::Ok(format!("Signed in (account {short}…, via {:?} TLS)", local.backend())),
    );

    observe(Step::Identity, StepState::Running);
    if chat.game_name.is_empty() {
        observe(
            Step::Identity,
            StepState::Warn("Your Riot ID isn't available yet".into()),
        );
    } else {
        observe(
            Step::Identity,
            StepState::Ok(format!("{}#{}", chat.game_name, chat.game_tag)),
        );
    }

    // §5 region & shard.
    observe(Step::Region, StepState::Running);
    let (region, shard, source) = match detect_region(&local, &chat, ent.access_token.expose()).await {
        Some(r) => r,
        None => {
            observe(
                Step::Region,
                StepState::failed(
                    "Couldn't work out your VALORANT region",
                    "Launch VALORANT itself; the Riot Client alone doesn't always report a region.",
                ),
            );
            return Err(RiotError::RegionUnknown);
        }
    };
    let hosts = match Hosts::new(region, shard) {
        Ok(h) => h,
        Err(e) => {
            observe(Step::Region, StepState::failed(e.to_string(), "Copy the diagnostics."));
            return Err(e);
        }
    };
    observe(
        Step::Region,
        StepState::Ok(format!(
            "{} (shard {shard}), {}",
            region.as_str().to_uppercase(),
            source.describe()
        )),
    );

    // §6 client version → public auth.
    observe(Step::ClientVersion, StepState::Running);
    let client_version = match version::fetch(http).await {
        Ok(v) => v,
        Err(e) => {
            observe(
                Step::ClientVersion,
                StepState::failed(
                    e.to_string(),
                    "Check your internet connection (the version comes from valorant-api.com, or VALORANT's log once the game has started).",
                ),
            );
            return Err(e);
        }
    };
    observe(Step::ClientVersion, StepState::Ok(client_version.clone()));

    // The test-only fake backend serves PD/GLZ on loopback with a self-signed
    // cert, so it needs the loopback-only TLS client.
    let http = if hosts.fake { loopback_http()? } else { http.clone() };
    tracing::info!(%region, %shard, "riot session established");
    Ok(RiotSession {
        puuid: chat.puuid,
        game_name: chat.game_name,
        tag_line: chat.game_tag,
        region,
        shard,
        hosts,
        local,
        public: RwLock::new(PublicAuth::new(ent.access_token, ent.token, client_version)),
        version_checked: Mutex::new(Instant::now()),
        http,
        limiter: Semaphore::new(MAX_CONCURRENT),
    })
}

fn local_failure(e: &ApiError) -> StepState {
    match e {
        ApiError::Unauthorized => StepState::failed(
            "The Riot Client rejected the lockfile password",
            "The client probably restarted; this retries automatically. If it keeps failing, restart the Riot Client.",
        ),
        ApiError::Network(detail) => StepState::failed(
            format!("Couldn't reach the Riot Client's local API: {detail}"),
            "The client may still be starting. If this doesn't clear up, check that security software isn't blocking connections to 127.0.0.1, then copy the diagnostics.",
        ),
        other => StepState::failed(
            format!("The Riot Client's local API returned an error: {other}"),
            "Restart the Riot Client. If it keeps failing, copy the diagnostics.",
        ),
    }
}

fn loopback_http() -> Result<reqwest::Client, RiotError> {
    reqwest::Client::builder()
        .use_preconfigured_tls((*super::tls::local_client_config()).clone())
        .no_proxy()
        .build()
        .map_err(|e| RiotError::Unexpected(e.to_string()))
}

/// Region + shard, most reliable source first: VALORANT's launch arguments,
/// the Riot Client's region, the chat session's region, then the region
/// claim inside the RSO access token (decoded locally, never sent anywhere).
pub async fn detect_region(
    local: &LocalClient,
    chat: &ChatSession,
    access_token: &str,
) -> Option<(Region, Shard, RegionSource)> {
    if let Ok(sessions) = local.external_sessions().await {
        for s in sessions.values().filter(|s| s.product_id == "valorant") {
            if let Some((r, sh)) = region::from_launch_args(&s.launch_configuration.arguments) {
                return Some((r, sh, RegionSource::LaunchArgs));
            }
        }
    }
    if let Ok(rl) = local.region_locale().await {
        if let Some(r) = [&rl.web_region, &rl.region]
            .into_iter()
            .find_map(|h| Region::from_hint(h))
        {
            return Some((r, r.default_shard(), RegionSource::ClientRegion));
        }
    }
    if let Some(r) = Region::from_hint(&chat.region) {
        return Some((r, r.default_shard(), RegionSource::ChatSession));
    }
    region_from_token(access_token).map(|r| (r, r.default_shard(), RegionSource::TokenClaim))
}

/// Look for a region-like claim in a JWT payload (`pp.c`, `dat.r`, …).
pub fn region_from_token(jwt: &str) -> Option<Region> {
    use base64::Engine;
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let candidates = [&v["pp"]["c"], &v["dat"]["r"], &v["region"], &v["affinity"]];
    candidates.iter().filter_map(|c| c.as_str()).find_map(Region::from_hint)
}

impl RiotSession {
    pub fn riot_id(&self) -> RiotId {
        RiotId {
            game_name: self.game_name.clone(),
            tag_line: self.tag_line.clone(),
        }
    }

    pub fn local_auth(&self) -> &LocalAuth {
        self.local.auth()
    }

    /// A snapshot of the current public auth context.
    pub async fn public(&self) -> PublicAuth {
        self.public.read().await.clone()
    }

    pub async fn client_version(&self) -> String {
        self.public.read().await.client_version.clone()
    }

    pub fn pd(&self) -> Result<String, ApiError> {
        Ok(self.hosts.pd.clone())
    }

    pub fn glz(&self) -> Result<String, ApiError> {
        Ok(self.hosts.glz.clone())
    }

    /// Refetch the client version; returns true if it changed.
    async fn refresh_version(&self) -> bool {
        *self.version_checked.lock().await = Instant::now();
        match version::fetch(&self.http).await {
            Ok(v) => {
                let mut public = self.public.write().await;
                let changed = public.client_version != v;
                if changed {
                    tracing::info!(old = %public.client_version, new = %v, "client version updated");
                    public.client_version = v;
                }
                changed
            }
            Err(e) => {
                tracing::warn!("version refresh failed: {e}");
                false
            }
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, ApiError> {
        self.send(url, |http| http.get(url)).await
    }

    pub async fn put<B: serde::Serialize, T: DeserializeOwned>(&self, url: &str, body: &B) -> Result<T, ApiError> {
        self.send(url, |http| http.put(url).json(body)).await
    }

    /// One authenticated call with the §8 recovery rules: 401/403 (or a
    /// `BAD_CLAIMS` 400) → refresh tokens and retry once; a version-looking
    /// 400 → refetch the client version and retry once; 429 → honour
    /// `Retry-After` (capped).
    async fn send<T, F>(&self, url: &str, build: F) -> Result<T, ApiError>
    where
        T: DeserializeOwned,
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        // Auth headers only ever go to Riot's own API hosts.
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let fake = self.hosts.fake && host == "127.0.0.1";
        if !region::is_riot_api_host(&host) && !fake {
            return Err(ApiError::InvalidHost(host));
        }
        if self.version_checked.lock().await.elapsed() > version::MAX_AGE {
            self.refresh_version().await;
        }

        let (mut tokens_refreshed, mut version_refreshed, mut retries) = (false, false, 0);
        loop {
            let public = self.public().await;
            let resp = {
                let _permit = self
                    .limiter
                    .acquire()
                    .await
                    .map_err(|_| ApiError::Other("shutting down".into()))?;
                public.apply(build(&self.http)).send().await?
            };
            let status = resp.status().as_u16();
            match status {
                200..=299 => return Ok(resp.json().await?),
                401 | 403 if !tokens_refreshed => {
                    tokens_refreshed = true;
                    refresh_tokens(self).await.map_err(ApiError::from)?;
                }
                400 => {
                    let body = resp.text().await.unwrap_or_default();
                    if body.contains("BAD_CLAIMS") && !tokens_refreshed {
                        tokens_refreshed = true;
                        refresh_tokens(self).await.map_err(ApiError::from)?;
                    } else if version::looks_like_version_error(&body) && !version_refreshed {
                        version_refreshed = true;
                        if !self.refresh_version().await {
                            return Err(ApiError::Status(400));
                        }
                    } else {
                        return Err(ApiError::Status(400));
                    }
                }
                401 | 403 => return Err(ApiError::Unauthorized),
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

/// Re-fetch tokens from the local API (the Riot client keeps them fresh
/// while it runs) and swap them into the session (brief §8).
///
/// Takes `&RiotSession` rather than `&mut`: the session is shared between
/// tasks, so the tokens sit behind a lock instead.
pub async fn refresh_tokens(session: &RiotSession) -> Result<(), RiotError> {
    let ent = session.local.entitlements().await.map_err(|e| match e {
        ApiError::Unauthorized | ApiError::NotFound => RiotError::AuthFailed,
        other => other.into(),
    })?;
    let mut public = session.public.write().await;
    public.access_token = ent.access_token;
    public.entitlement = ent.token;
    tracing::info!("riot tokens refreshed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn region_claim_in_token() {
        let enc = |v: serde_json::Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
        let jwt = |payload| format!("{}.{}.sig", enc(serde_json::json!({"alg": "RS256"})), enc(payload));
        assert_eq!(
            region_from_token(&jwt(serde_json::json!({"pp": {"c": "eu"}}))),
            Some(Region::Eu)
        );
        assert_eq!(
            region_from_token(&jwt(serde_json::json!({"dat": {"r": "NA1"}}))),
            Some(Region::Na)
        );
        assert_eq!(region_from_token(&jwt(serde_json::json!({"sub": "x"}))), None);
        assert_eq!(region_from_token("not-a-jwt"), None);
    }
}
