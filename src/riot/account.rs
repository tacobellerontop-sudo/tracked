//! Optional name#tag -> PUUID resolution through Riot's *official* developer
//! API (`account-v1`). Only used when the user provides their own key via the
//! `RIOT_API_KEY` environment variable; the key is never stored or logged.

use serde::Deserialize;

use crate::error::ApiError;
use crate::secret::Secret;

pub fn api_key() -> Option<Secret<String>> {
    std::env::var("RIOT_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .map(Secret::new)
}

/// Regional routing value for account-v1.
pub fn routing_for(region: &str) -> &'static str {
    match region {
        "eu" => "europe",
        "ap" | "kr" => "asia",
        _ => "americas",
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    puuid: String,
}

pub async fn puuid_by_riot_id(
    http: &reqwest::Client,
    region: &str,
    name: &str,
    tag: &str,
    key: &Secret<String>,
) -> Result<String, ApiError> {
    let mut url = reqwest::Url::parse(&format!("https://{}.api.riotgames.com/", routing_for(region)))
        .map_err(|e| ApiError::Other(e.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| ApiError::Other("bad url".into()))?
        .extend(["riot", "account", "v1", "accounts", "by-riot-id", name, tag]);
    let mut value =
        reqwest::header::HeaderValue::from_str(key.expose()).map_err(|_| ApiError::Other("invalid API key".into()))?;
    value.set_sensitive(true);
    let resp = http.get(url).header("X-Riot-Token", value).send().await?;
    match resp.status().as_u16() {
        200 => Ok(resp.json::<Account>().await?.puuid),
        404 => Err(ApiError::NotFound),
        401 | 403 => Err(ApiError::Other("RIOT_API_KEY was rejected (expired?)".into())),
        429 => Err(ApiError::RateLimited(None)),
        s => Err(ApiError::Status(s)),
    }
}
