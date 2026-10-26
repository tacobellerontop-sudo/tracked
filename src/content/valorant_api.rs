//! Client for `https://valorant-api.com` plus an on-disk icon cache.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::error::ApiError;

const BASE: &str = "https://valorant-api.com";

#[derive(Debug, Clone, Default)]
pub struct Agent {
    pub name: String,
    pub icon: String,
}

#[derive(Debug, Clone, Default)]
pub struct MapInfo {
    pub name: String,
    pub icon: String,
    /// Large 16:9 artwork (loaded on demand, not pre-cached).
    pub splash: String,
}

#[derive(Debug, Clone, Default)]
pub struct Tier {
    pub name: String,
    pub icon: String,
    /// `RRGGBBAA` hex from valorant-api.
    pub color: String,
}

/// Everything the UI needs to turn ids into names and image URIs.
/// Image fields are `file://` URIs once cached, otherwise `https://` URLs.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub client_version: Option<String>,
    /// Keyed by lowercase agent UUID.
    pub agents: HashMap<String, Agent>,
    /// Keyed by lowercase map UUID *and* by its `/Game/Maps/...` path.
    pub maps: HashMap<String, MapInfo>,
    pub tiers: HashMap<u32, Tier>,
    /// UUID of the act that is live right now.
    pub current_act: Option<String>,
    /// e.g. "Episode 9 · Act III".
    pub current_act_name: Option<String>,
}

impl Catalog {
    pub fn agent(&self, id: &str) -> Option<&Agent> {
        self.agents.get(&id.to_ascii_lowercase())
    }

    pub fn map(&self, id: &str) -> Option<&MapInfo> {
        self.maps.get(id).or_else(|| self.maps.get(&id.to_ascii_lowercase()))
    }

    pub fn map_name(&self, id: &str) -> String {
        self.map(id).map(|m| m.name.clone()).unwrap_or_else(|| {
            // "/Game/Maps/Ascent/Ascent" -> "Ascent" as a last resort.
            id.rsplit('/').next().unwrap_or(id).to_string()
        })
    }

    pub fn tier(&self, tier: u32) -> Option<&Tier> {
        self.tiers.get(&tier)
    }

    pub fn tier_name(&self, tier: u32) -> String {
        match self.tiers.get(&tier) {
            Some(t) => t.name.clone(),
            None if tier == 0 => "Unrated".into(),
            None => format!("Tier {tier}"),
        }
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

async fn get<T: for<'de> Deserialize<'de>>(http: &reqwest::Client, path: &str) -> Result<T, ApiError> {
    let resp = http.get(format!("{BASE}{path}")).send().await?;
    if !resp.status().is_success() {
        return Err(ApiError::Status(resp.status().as_u16()));
    }
    Ok(resp.json::<Envelope<T>>().await?.data)
}

pub async fn client_version(http: &reqwest::Client) -> Result<String, ApiError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct V {
        riot_client_version: String,
    }
    Ok(get::<V>(http, "/v1/version").await?.riot_client_version)
}

fn str_of(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Fetch the full catalog. Individual sections that fail are left empty.
pub async fn fetch_catalog(http: &reqwest::Client) -> Result<Catalog, ApiError> {
    let (version, agents, maps, tiers, seasons) = tokio::join!(
        client_version(http),
        get::<Vec<Value>>(http, "/v1/agents?isPlayableCharacter=true"),
        get::<Vec<Value>>(http, "/v1/maps"),
        get::<Vec<Value>>(http, "/v1/competitivetiers"),
        get::<Vec<Value>>(http, "/v1/seasons"),
    );
    if let (Err(e), Err(_), Err(_)) = (&version, &agents, &maps) {
        return Err(ApiError::Network(e.to_string()));
    }
    let mut c = Catalog {
        client_version: version.ok(),
        ..Default::default()
    };

    for a in agents.unwrap_or_default() {
        c.agents.insert(
            str_of(&a, "uuid").to_ascii_lowercase(),
            Agent {
                name: str_of(&a, "displayName"),
                icon: str_of(&a, "displayIconSmall"),
            },
        );
    }
    for m in maps.unwrap_or_default() {
        let info = MapInfo {
            name: str_of(&m, "displayName"),
            icon: str_of(&m, "listViewIcon"),
            splash: str_of(&m, "splash"),
        };
        c.maps.insert(str_of(&m, "mapUrl"), info.clone());
        c.maps.insert(str_of(&m, "uuid").to_ascii_lowercase(), info);
    }
    // The last tier table is the current episode's.
    if let Some(table) = tiers.unwrap_or_default().last() {
        for t in table.get("tiers").and_then(Value::as_array).into_iter().flatten() {
            let Some(n) = t.get("tier").and_then(Value::as_u64) else {
                continue;
            };
            c.tiers.insert(
                n as u32,
                Tier {
                    name: title_case(&str_of(t, "tierName")),
                    icon: str_of(t, "smallIcon"),
                    color: str_of(t, "color"),
                },
            );
        }
    }
    let seasons = seasons.unwrap_or_default();
    c.current_act = current_act(&seasons, chrono::Utc::now());
    c.current_act_name = c.current_act.as_deref().and_then(|a| act_name(&seasons, a));
    Ok(c)
}

/// "Episode 9 · Act III" from the seasons list.
pub fn act_name(seasons: &[Value], act: &str) -> Option<String> {
    let find = |id: &str| seasons.iter().find(|s| str_of(s, "uuid").eq_ignore_ascii_case(id));
    let a = find(act)?;
    let act_name = title_case(&str_of(a, "displayName"));
    match find(&str_of(a, "parentUuid")).map(|e| title_case(&str_of(e, "displayName"))) {
        Some(ep) if !ep.is_empty() => Some(format!("{ep} · {act_name}")),
        _ => Some(act_name),
    }
}

/// Player card art: (square icon, wide banner art).
pub async fn player_card(http: &reqwest::Client, id: &str) -> Result<(String, String), ApiError> {
    let v: Value = get(http, &format!("/v1/playercards/{id}")).await?;
    Ok((str_of(&v, "displayIcon"), str_of(&v, "wideArt")))
}

pub async fn player_title(http: &reqwest::Client, id: &str) -> Result<String, ApiError> {
    let v: Value = get(http, &format!("/v1/playertitles/{id}")).await?;
    Ok(str_of(&v, "titleText"))
}

/// The act whose `[startTime, endTime)` contains `now`.
pub fn current_act(seasons: &[Value], now: chrono::DateTime<chrono::Utc>) -> Option<String> {
    seasons.iter().find_map(|s| {
        if str_of(s, "type") != "EAresSeasonType::Act" {
            return None;
        }
        let start = chrono::DateTime::parse_from_rfc3339(&str_of(s, "startTime")).ok()?;
        let end = chrono::DateTime::parse_from_rfc3339(&str_of(s, "endTime")).ok()?;
        (start <= now && now < end).then(|| str_of(s, "uuid").to_ascii_lowercase())
    })
}

fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().chain(cs.flat_map(char::to_lowercase)).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Download every icon in the catalog into `dir` (skipping ones already
/// there) and rewrite the catalog to point at the local copies.
pub async fn cache_icons(http: &reqwest::Client, mut c: Catalog, dir: &Path) -> Catalog {
    if tokio::fs::create_dir_all(dir).await.is_err() {
        return c;
    }
    let urls: Vec<String> = c
        .agents
        .values()
        .map(|a| a.icon.clone())
        .chain(c.maps.values().map(|m| m.icon.clone()))
        .chain(c.tiers.values().map(|t| t.icon.clone()))
        .filter(|u| u.starts_with("https://"))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(6));
    let mut tasks = Vec::new();
    for url in urls {
        let path = icon_path(dir, &url);
        if path.exists() {
            continue;
        }
        let (http, sem) = (http.clone(), sem.clone());
        tasks.push(tokio::spawn(async move {
            let _p = sem.acquire().await;
            if let Ok(resp) = http.get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(bytes) = resp.bytes().await {
                        let tmp = path.with_extension("part");
                        if tokio::fs::write(&tmp, &bytes).await.is_ok() {
                            let _ = tokio::fs::rename(&tmp, &path).await;
                        }
                    }
                }
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }

    let local = |u: &mut String| {
        let p = icon_path(dir, u);
        if p.exists() {
            *u = format!("file://{}", p.display());
        }
    };
    c.agents.values_mut().for_each(|a| local(&mut a.icon));
    c.maps.values_mut().for_each(|m| local(&mut m.icon));
    c.tiers.values_mut().for_each(|t| local(&mut t.icon));
    c
}

/// Stable file name for a URL (FNV-1a hash).
pub fn icon_path(dir: &Path, url: &str) -> PathBuf {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in url.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    dir.join(format!("{h:016x}.png"))
}

/// Human name for a queue id.
pub fn queue_name(queue: &str) -> &str {
    match queue {
        "competitive" => "Competitive",
        "unrated" => "Unrated",
        "swiftplay" => "Swiftplay",
        "spikerush" => "Spike Rush",
        "deathmatch" => "Deathmatch",
        "ggteam" => "Escalation",
        "onefa" => "Replication",
        "hurm" => "Team Deathmatch",
        "premier" => "Premier",
        "newmap" => "New Map",
        "" => "Custom",
        other => other,
    }
}
