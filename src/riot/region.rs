//! Region/shard detection and host construction.

use std::sync::OnceLock;

use regex::Regex;

use crate::error::ApiError;

pub const KNOWN_REGIONS: &[&str] = &["na", "latam", "br", "eu", "ap", "kr"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub region: String,
    pub shard: String,
}

impl Region {
    /// Validate a region and derive its shard (`latam`/`br` live on `na`).
    pub fn new(region: &str) -> Result<Self, ApiError> {
        let region = region.trim().to_ascii_lowercase();
        if !KNOWN_REGIONS.contains(&region.as_str()) {
            return Err(ApiError::RegionUnknown);
        }
        let shard = match region.as_str() {
            "latam" | "br" => "na".to_string(),
            r => r.to_string(),
        };
        Ok(Self { region, shard })
    }

    /// `https://pd.<shard>.a.pvp.net`
    pub fn pd_base(&self) -> Result<String, ApiError> {
        let host = format!("pd.{}.a.pvp.net", self.shard);
        if !pd_regex().is_match(&host) {
            return Err(ApiError::InvalidHost(host));
        }
        Ok(format!("https://{host}"))
    }

    /// `https://glz-<region>-1.<shard>.a.pvp.net`, validated.
    pub fn glz_base(&self) -> Result<String, ApiError> {
        let host = format!("glz-{}-1.{}.a.pvp.net", self.region, self.shard);
        if !is_valid_glz_host(&host) {
            return Err(ApiError::InvalidHost(host));
        }
        Ok(format!("https://{host}"))
    }
}

fn glz_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^glz-([a-z]+)-\d+\.([a-z]+)\.a\.pvp\.net$").unwrap())
}

fn pd_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^pd\.([a-z]+)\.a\.pvp\.net$").unwrap())
}

pub fn is_valid_glz_host(host: &str) -> bool {
    glz_regex().is_match(host)
}

/// True for hosts that may receive Riot auth headers.
pub fn is_riot_api_host(host: &str) -> bool {
    pd_regex().is_match(host) || is_valid_glz_host(host)
}

/// Pull `-ares-deployment=<region>` out of VALORANT's launch arguments.
pub fn region_from_launch_args<S: AsRef<str>>(args: &[S]) -> Option<String> {
    args.iter()
        .find_map(|a| a.as_ref().strip_prefix("-ares-deployment="))
        .map(|r| r.to_ascii_lowercase())
}
