//! REST client for the backend, configurable base URL, bearer auth.

use std::sync::RwLock;
use std::time::Duration;

use reqwest::{Method, Url};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use super::models::*;
use crate::error::ApiError;
use crate::secret::Secret;

pub struct BackendClient {
    base: Url,
    http: reqwest::Client,
    token: RwLock<Option<Secret<String>>>,
}

impl std::fmt::Debug for BackendClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendClient")
            .field("base", &self.base.as_str())
            .finish_non_exhaustive()
    }
}

/// Accept `https://…`, or plain `http://` only for loopback (the local mock).
/// No credentials, query or fragment in the URL.
pub fn validate_base(raw: &str) -> Result<Url, ApiError> {
    let bad = |m: &str| ApiError::Other(format!("Backend URL {m}"));
    let url = Url::parse(raw.trim()).map_err(|_| bad("is not a valid URL"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad("must not contain credentials"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(bad("must not contain a query or fragment"));
    }
    let host = url.host_str().ok_or_else(|| bad("has no host"))?;
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    match url.scheme() {
        "https" => Ok(url),
        "http" if loopback => Ok(url),
        "http" => Err(bad("must use https (http is only allowed for localhost)")),
        _ => Err(bad("must be http(s)")),
    }
}

#[derive(Deserialize)]
struct LoginResp {
    token: String,
    user: BackendUser,
}

impl BackendClient {
    pub fn new(base: &str) -> Result<Self, ApiError> {
        let mut base = validate_base(base)?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let loopback = base.scheme() == "http";
        let mut builder = reqwest::Client::builder()
            .user_agent(concat!("tracked/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(10));
        if loopback {
            builder = builder.no_proxy();
        }
        let http = builder.build().map_err(|e| ApiError::Other(e.to_string()))?;
        Ok(Self {
            base,
            http,
            token: RwLock::new(None),
        })
    }

    pub fn base(&self) -> &Url {
        &self.base
    }

    pub fn signed_in(&self) -> bool {
        self.token.read().unwrap().is_some()
    }

    pub fn token(&self) -> Option<Secret<String>> {
        self.token.read().unwrap().clone()
    }

    pub fn sign_out(&self) {
        *self.token.write().unwrap() = None;
    }

    /// `ws(s)://<base>/social/ws`
    pub fn ws_url(&self) -> Url {
        let mut url = self.base.join("social/ws").expect("valid path");
        let scheme = if self.base.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme).expect("ws scheme");
        url
    }

    async fn call<T: DeserializeOwned>(&self, method: Method, path: &str, body: Option<Value>) -> Result<T, ApiError> {
        let url = self.base.join(path).map_err(|e| ApiError::Other(e.to_string()))?;
        let mut req = self.http.request(method, url);
        if let Some(token) = self.token() {
            let mut v = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token.expose()))
                .map_err(|_| ApiError::Other("invalid token".into()))?;
            v.set_sensitive(true);
            req = req.header(reqwest::header::AUTHORIZATION, v);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await?;
        match resp.status().as_u16() {
            200..=299 => Ok(resp.json().await?),
            401 => Err(ApiError::Unauthorized),
            404 => Err(ApiError::NotFound),
            429 => Err(ApiError::RateLimited(None)),
            s => {
                let msg = resp
                    .json::<Value>()
                    .await
                    .ok()
                    .and_then(|v| v["error"].as_str().map(str::to_string));
                Err(msg.map(ApiError::Other).unwrap_or(ApiError::Status(s)))
            }
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        self.call(Method::GET, path, None).await
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T, ApiError> {
        self.call(Method::POST, path, Some(body)).await
    }

    // ---- auth

    /// Development login against the mock (or a compatible issuer). The JWT
    /// is held in memory only and never persisted.
    pub async fn dev_login(&self, display_name: &str, riot_id: Option<&str>) -> Result<BackendUser, ApiError> {
        let r: LoginResp = self
            .post(
                "api/auth/dev-login",
                json!({ "display_name": display_name, "riot_id": riot_id }),
            )
            .await?;
        *self.token.write().unwrap() = Some(Secret::new(r.token));
        Ok(r.user)
    }

    // ---- companion

    pub async fn app_config(&self) -> Result<AppConfig, ApiError> {
        self.get("api/companion/app-config").await
    }

    pub async fn min_version(&self) -> Result<MinVersion, ApiError> {
        self.get("api/companion/min-version").await
    }

    pub async fn heartbeat(&self, state: &str) -> Result<Value, ApiError> {
        self.post(
            "api/companion/heartbeat",
            json!({ "version": env!("CARGO_PKG_VERSION"), "state": state }),
        )
        .await
    }

    pub async fn feedback(&self, text: &str) -> Result<Value, ApiError> {
        self.post(
            "api/companion/feedback",
            json!({ "text": text, "version": env!("CARGO_PKG_VERSION") }),
        )
        .await
    }

    pub async fn lobby_push(&self, lobby: &LobbyInfo) -> Result<Value, ApiError> {
        self.post("api/companion/lobby", serde_json::to_value(lobby)?).await
    }

    pub async fn lobby_touch(&self) -> Result<Value, ApiError> {
        self.post("api/companion/lobby/touch", json!({})).await
    }

    pub async fn lobby_clear(&self) -> Result<Value, ApiError> {
        self.post("api/companion/lobby/clear", json!({})).await
    }

    pub async fn lobby_lookup(&self, user_ids: &[String]) -> Result<Vec<(String, LobbyInfo)>, ApiError> {
        #[derive(Deserialize)]
        struct Item {
            user_id: String,
            lobby: LobbyInfo,
        }
        #[derive(Deserialize)]
        struct R {
            lobbies: Vec<Item>,
        }
        let r: R = self
            .post("api/companion/lobby/lookup", json!({ "user_ids": user_ids }))
            .await?;
        Ok(r.lobbies.into_iter().map(|i| (i.user_id, i.lobby)).collect())
    }

    pub async fn presence_lookup(&self, user_ids: &[String]) -> Result<Vec<PresenceEntry>, ApiError> {
        #[derive(Deserialize)]
        struct R {
            entries: Vec<PresenceEntry>,
        }
        let r: R = self
            .post("api/companion/presence/lookup", json!({ "user_ids": user_ids }))
            .await?;
        Ok(r.entries)
    }

    pub async fn account(&self) -> Result<Value, ApiError> {
        self.get("api/account").await
    }

    pub async fn crosshairs(&self) -> Result<Value, ApiError> {
        self.get("api/crosshairs").await
    }

    // ---- social

    pub async fn friends(&self) -> Result<Vec<BackendUser>, ApiError> {
        #[derive(Deserialize)]
        struct R {
            friends: Vec<BackendUser>,
        }
        Ok(self.get::<R>("social/v1/friends").await?.friends)
    }

    pub async fn dm_list(&self) -> Result<Vec<Conversation>, ApiError> {
        #[derive(Deserialize)]
        struct R {
            conversations: Vec<Conversation>,
        }
        Ok(self.get::<R>("social/v1/dm").await?.conversations)
    }

    pub async fn dm_page(&self, conv: &str, before: Option<u64>, limit: u32) -> Result<DmPage, ApiError> {
        let mut path = format!("social/v1/dm/{}/messages?limit={limit}", seg(conv)?);
        if let Some(b) = before {
            path.push_str(&format!("&before={b}"));
        }
        self.get(&path).await
    }

    pub async fn dm_send(&self, conv: &str, text: &str) -> Result<DmMessage, ApiError> {
        self.post(
            &format!("social/v1/dm/{}/messages", seg(conv)?),
            json!({ "text": text }),
        )
        .await
    }

    pub async fn dm_create_group(&self, members: &[String], name: Option<&str>) -> Result<Conversation, ApiError> {
        self.post("social/v1/dm", json!({ "members": members, "name": name }))
            .await
    }

    pub async fn dm_rename(&self, conv: &str, name: &str) -> Result<Conversation, ApiError> {
        self.post(&format!("social/v1/dm/{}/rename", seg(conv)?), json!({ "name": name }))
            .await
    }

    pub async fn dm_mark_read(&self, conv: &str, message_id: u64) -> Result<Value, ApiError> {
        self.post(
            &format!("social/v1/dm/{}/read", seg(conv)?),
            json!({ "message_id": message_id }),
        )
        .await
    }

    pub async fn dm_typing(&self, conv: &str) -> Result<Value, ApiError> {
        self.post(&format!("social/v1/dm/{}/typing", seg(conv)?), json!({}))
            .await
    }

    pub async fn dm_report(&self, conv: &str, message_id: u64, reason: &str) -> Result<Value, ApiError> {
        self.post(
            &format!("social/v1/dm/{}/report", seg(conv)?),
            json!({ "message_id": message_id, "reason": reason }),
        )
        .await
    }

    pub async fn dm_request_respond(&self, request_id: &str, accept: bool) -> Result<Value, ApiError> {
        self.post(
            &format!("social/v1/dm/requests/{}", seg(request_id)?),
            json!({ "accept": accept }),
        )
        .await
    }
}

/// Ids go into URL paths: allow only a conservative character set.
fn seg(id: &str) -> Result<&str, ApiError> {
    let ok = !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then_some(id).ok_or_else(|| ApiError::Other("invalid id".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_validation() {
        assert!(validate_base("https://api.example.com").is_ok());
        assert!(validate_base("http://127.0.0.1:8787").is_ok());
        assert!(validate_base("http://localhost:8787/").is_ok());
        assert!(validate_base("http://example.com").is_err());
        assert!(validate_base("https://user:pw@example.com").is_err());
        assert!(validate_base("https://example.com/?x=1").is_err());
        assert!(validate_base("ftp://example.com").is_err());
        assert!(validate_base("not a url").is_err());
    }

    #[test]
    fn ws_url_follows_scheme() {
        let c = BackendClient::new("http://127.0.0.1:8787").unwrap();
        assert_eq!(c.ws_url().as_str(), "ws://127.0.0.1:8787/social/ws");
        let c = BackendClient::new("https://example.com/base").unwrap();
        assert_eq!(c.ws_url().as_str(), "wss://example.com/base/social/ws");
    }

    #[test]
    fn path_ids_are_checked() {
        assert!(seg("dm-123_x").is_ok());
        assert!(seg("../x").is_err());
        assert!(seg("a/b").is_err());
        assert!(seg("").is_err());
    }
}
