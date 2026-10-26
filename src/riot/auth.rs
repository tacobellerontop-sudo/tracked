//! Tokens and the header set required by Riot's PD/GLZ endpoints.

use base64::Engine;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};

use crate::error::ApiError;
use crate::secret::Secret;

/// The platform descriptor Riot expects, before base64.
pub const CLIENT_PLATFORM_JSON: &str = r#"{"platformType":"PC","platformOS":"Windows","platformOSVersion":"10.0.19043.1.256.64bit","platformChipset":"Unknown"}"#;

pub fn client_platform_header() -> String {
    base64::engine::general_purpose::STANDARD.encode(CLIENT_PLATFORM_JSON)
}

/// Everything needed to authenticate against PD/GLZ. Held in memory only.
#[derive(Debug, Clone)]
pub struct RiotAuth {
    pub access_token: Secret<String>,
    pub entitlements_jwt: Secret<String>,
    pub puuid: String,
    pub client_version: String,
}

impl RiotAuth {
    /// Build the four required headers. Token-bearing values are marked
    /// sensitive so reqwest/hyper never print them.
    pub fn headers(&self) -> Result<HeaderMap, ApiError> {
        let bad = |what: &str| ApiError::Other(format!("invalid {what} header value"));
        let mut h = HeaderMap::new();

        let mut bearer = HeaderValue::from_str(&format!("Bearer {}", self.access_token.expose()))
            .map_err(|_| bad("authorization"))?;
        bearer.set_sensitive(true);
        h.insert(AUTHORIZATION, bearer);

        let mut ent = HeaderValue::from_str(self.entitlements_jwt.expose()).map_err(|_| bad("entitlements"))?;
        ent.set_sensitive(true);
        h.insert(HeaderName::from_static("x-riot-entitlements-jwt"), ent);

        h.insert(
            HeaderName::from_static("x-riot-clientplatform"),
            HeaderValue::from_str(&client_platform_header()).map_err(|_| bad("platform"))?,
        );
        h.insert(
            HeaderName::from_static("x-riot-clientversion"),
            HeaderValue::from_str(&self.client_version).map_err(|_| bad("client version"))?,
        );
        Ok(h)
    }
}
