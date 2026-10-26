//! Region/shard detection and host construction.

use std::sync::OnceLock;

use regex::Regex;

use crate::error::ApiError;

pub const KNOWN_REGIONS: &[&str] = &["na", "latam", "br", "eu", "ap", "kr"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub region: String,
    pub shard: String,
    /// Test-only loopback stand-in for PD/GLZ (feature `fake-riot`).
    pub fake_base: Option<String>,
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
        Ok(Self {
            region,
            shard,
            fake_base: fake_base(),
        })
    }

    /// `https://pd.<shard>.a.pvp.net`
    pub fn pd_base(&self) -> Result<String, ApiError> {
        if let Some(b) = &self.fake_base {
            return Ok(b.clone());
        }
        let host = format!("pd.{}.a.pvp.net", self.shard);
        if !pd_regex().is_match(&host) {
            return Err(ApiError::InvalidHost(host));
        }
        Ok(format!("https://{host}"))
    }

    /// `https://glz-<region>-1.<shard>.a.pvp.net`, validated.
    pub fn glz_base(&self) -> Result<String, ApiError> {
        if let Some(b) = &self.fake_base {
            return Ok(b.clone());
        }
        let host = format!("glz-{}-1.{}.a.pvp.net", self.region, self.shard);
        if !is_valid_glz_host(&host) {
            return Err(ApiError::InvalidHost(host));
        }
        Ok(format!("https://{host}"))
    }
}

/// `ASCEND_FAKE_RIOT_BASE`, honoured only with the `fake-riot` feature and
/// only for a loopback `https://127.0.0.1:<port>` URL.
#[cfg(feature = "fake-riot")]
fn fake_base() -> Option<String> {
    let v = std::env::var("ASCEND_FAKE_RIOT_BASE").ok()?;
    let port = v.strip_prefix("https://127.0.0.1:")?;
    port.parse::<u16>().ok().map(|_| v)
}

#[cfg(not(feature = "fake-riot"))]
fn fake_base() -> Option<String> {
    None
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

/// Map the many ways Riot names a region ("EUW", "eu3", "oc1", "LA1", "NA",
/// "shared.eu.a.pvp.net" shard names…) onto a VALORANT region.
pub fn region_from_hint(hint: &str) -> Option<String> {
    let h = hint.trim().to_ascii_lowercase();
    let h = h.trim_end_matches(|c: char| c.is_ascii_digit());
    let r = match h {
        "na" | "pbe" => "na",
        "eu" | "euw" | "eune" | "tr" | "ru" | "me" => "eu",
        "ap" | "oc" | "oce" | "jp" | "sg" | "ph" | "th" | "tw" | "vn" | "sea" => "ap",
        "kr" => "kr",
        "br" => "br",
        "latam" | "la" | "lan" | "las" => "latam",
        _ => return None,
    };
    Some(r.to_string())
}

/// `-config-endpoint=https://shared.<shard>.a.pvp.net` -> shard.
pub fn shard_from_launch_args<S: AsRef<str>>(args: &[S]) -> Option<String> {
    args.iter().find_map(|a| {
        let url = a.as_ref().strip_prefix("-config-endpoint=")?;
        let host = url.trim_start_matches("https://").split('/').next()?;
        let shard = host.strip_prefix("shared.")?.strip_suffix(".a.pvp.net")?;
        Some(shard.to_ascii_lowercase())
    })
}

/// Pull `-ares-deployment=<region>` out of VALORANT's launch arguments.
pub fn region_from_launch_args<S: AsRef<str>>(args: &[S]) -> Option<String> {
    args.iter()
        .find_map(|a| a.as_ref().strip_prefix("-ares-deployment="))
        .map(|r| r.to_ascii_lowercase())
}
