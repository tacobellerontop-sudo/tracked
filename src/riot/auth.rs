//! The four headers every `*.pvp.net` call carries (brief §6).

use base64::Engine;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};

use crate::error::ApiError;
use crate::secret::Secret;

/// The platform descriptor Riot expects, before base64.
pub const CLIENT_PLATFORM_JSON: &str = r#"{"platformType":"PC","platformOS":"Windows","platformOSVersion":"10.0.19043.1.256.64bit","platformChipset":"Unknown"}"#;

/// `base64(CLIENT_PLATFORM_JSON)`, standard alphabet, no newlines.
pub fn client_platform_header() -> String {
    base64::engine::general_purpose::STANDARD.encode(CLIENT_PLATFORM_JSON)
}

/// Auth context for Riot's public endpoints. Held in memory only.
#[derive(Debug, Clone)]
pub struct PublicAuth {
    /// RSO access token (`Authorization: Bearer …`).
    pub access_token: Secret<String>,
    /// Entitlements JWT (`X-Riot-Entitlements-JWT`).
    pub entitlement: Secret<String>,
    /// base64 platform blob (`X-Riot-ClientPlatform`).
    pub client_platform: String,
    /// `X-Riot-ClientVersion`.
    pub client_version: String,
}

impl PublicAuth {
    pub fn new(access_token: Secret<String>, entitlement: Secret<String>, client_version: String) -> Self {
        Self {
            access_token,
            entitlement,
            client_platform: client_platform_header(),
            client_version,
        }
    }

    /// The four headers. Token-bearing values are marked sensitive so
    /// reqwest/hyper never print them.
    pub fn headers(&self) -> Result<HeaderMap, ApiError> {
        let bad = |what: &str| ApiError::Other(format!("invalid {what} header value"));
        let mut h = HeaderMap::new();

        let mut bearer = HeaderValue::from_str(&format!("Bearer {}", self.access_token.expose()))
            .map_err(|_| bad("authorization"))?;
        bearer.set_sensitive(true);
        h.insert(AUTHORIZATION, bearer);

        let mut ent = HeaderValue::from_str(self.entitlement.expose()).map_err(|_| bad("entitlements"))?;
        ent.set_sensitive(true);
        h.insert(HeaderName::from_static("x-riot-entitlements-jwt"), ent);

        h.insert(
            HeaderName::from_static("x-riot-clientplatform"),
            HeaderValue::from_str(&self.client_platform).map_err(|_| bad("platform"))?,
        );
        h.insert(
            HeaderName::from_static("x-riot-clientversion"),
            HeaderValue::from_str(&self.client_version).map_err(|_| bad("client version"))?,
        );
        Ok(h)
    }

    /// Set the four headers on a request.
    pub fn apply(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.headers() {
            Ok(h) => rb.headers(h),
            Err(e) => {
                // Only possible with a corrupt token; the request then fails
                // with 401 and triggers a token refresh.
                tracing::warn!("couldn't build Riot auth headers: {e}");
                rb
            }
        }
    }
}
