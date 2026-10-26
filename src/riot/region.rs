//! Regions, shards and the PD/GLZ host builders (brief §5).
//!
//! Region → shard (per the community reference): `latam` and `br` live on the
//! `na` shard; `na` can also be the `pbe` shard (the public test server);
//! everything else maps to itself.

use std::sync::OnceLock;

use regex::Regex;

use super::error::RiotError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Region {
    Na,
    Latam,
    Br,
    Eu,
    Ap,
    Kr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shard {
    Na,
    Pbe,
    Eu,
    Ap,
    Kr,
}

impl Region {
    pub const ALL: [Region; 6] = [
        Region::Na,
        Region::Latam,
        Region::Br,
        Region::Eu,
        Region::Ap,
        Region::Kr,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Region::Na => "na",
            Region::Latam => "latam",
            Region::Br => "br",
            Region::Eu => "eu",
            Region::Ap => "ap",
            Region::Kr => "kr",
        }
    }

    /// Exact VALORANT region code ("na", "latam", …), case-insensitive.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|r| r.as_str() == s)
    }

    /// Map the many ways Riot names a region ("EUW", "eu3", "OC1", "LA1",
    /// "pbe"…) onto a VALORANT region.
    pub fn from_hint(hint: &str) -> Option<Self> {
        let h = hint.trim().to_ascii_lowercase();
        let h = h.trim_end_matches(|c: char| c.is_ascii_digit());
        Some(match h {
            "na" | "pbe" => Region::Na,
            "eu" | "euw" | "eune" | "tr" | "ru" | "me" => Region::Eu,
            "ap" | "oc" | "oce" | "jp" | "sg" | "ph" | "th" | "tw" | "vn" | "sea" => Region::Ap,
            "kr" => Region::Kr,
            "br" => Region::Br,
            "latam" | "la" | "lan" | "las" => Region::Latam,
            _ => return None,
        })
    }

    /// The shard this region normally lives on.
    pub fn default_shard(self) -> Shard {
        match self {
            Region::Na | Region::Latam | Region::Br => Shard::Na,
            Region::Eu => Shard::Eu,
            Region::Ap => Shard::Ap,
            Region::Kr => Shard::Kr,
        }
    }
}

impl Shard {
    pub fn as_str(self) -> &'static str {
        match self {
            Shard::Na => "na",
            Shard::Pbe => "pbe",
            Shard::Eu => "eu",
            Shard::Ap => "ap",
            Shard::Kr => "kr",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        [Shard::Na, Shard::Pbe, Shard::Eu, Shard::Ap, Shard::Kr]
            .into_iter()
            .find(|x| x.as_str() == s)
    }

    /// Is this a valid shard for `region`?
    pub fn fits(self, region: Region) -> bool {
        self == region.default_shard() || (self == Shard::Pbe && region == Region::Na)
    }
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Display for Shard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Base URLs for the public player-data and game-lifecycle services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hosts {
    /// `https://pd.<shard>.a.pvp.net`
    pub pd: String,
    /// `https://glz-<region>-1.<shard>.a.pvp.net`
    pub glz: String,
    /// Test-only loopback stand-in for PD/GLZ (feature `fake-riot`).
    pub fake: bool,
}

impl Hosts {
    /// Build and validate both hosts, so a bad region can never produce a
    /// malformed request.
    pub fn new(region: Region, shard: Shard) -> Result<Self, RiotError> {
        if let Some(base) = fake_base() {
            return Ok(Self {
                pd: base.clone(),
                glz: base,
                fake: true,
            });
        }
        if !shard.fits(region) {
            return Err(RiotError::Unexpected(format!(
                "shard {shard} doesn't belong to region {region}"
            )));
        }
        let pd = format!("pd.{shard}.a.pvp.net");
        let glz = format!("glz-{region}-1.{shard}.a.pvp.net");
        if !pd_regex().is_match(&pd) || !is_valid_glz_host(&glz) {
            return Err(RiotError::Unexpected(format!(
                "refusing malformed Riot host for {region}/{shard}"
            )));
        }
        Ok(Self {
            pd: format!("https://{pd}"),
            glz: format!("https://{glz}"),
            fake: false,
        })
    }
}

/// `TRACKED_FAKE_RIOT_BASE`, honoured only with the `fake-riot` feature and
/// only for a loopback `https://127.0.0.1:<port>` URL.
#[cfg(feature = "fake-riot")]
fn fake_base() -> Option<String> {
    let v = std::env::var("TRACKED_FAKE_RIOT_BASE").ok()?;
    let port = v.strip_prefix("https://127.0.0.1:")?;
    port.parse::<u16>().ok().map(|_| v)
}

#[cfg(not(feature = "fake-riot"))]
fn fake_base() -> Option<String> {
    None
}

fn glz_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^glz-[a-z]+-\d+\.[a-z]+\.a\.pvp\.net$").unwrap())
}

fn pd_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^pd\.[a-z]+\.a\.pvp\.net$").unwrap())
}

pub fn is_valid_glz_host(host: &str) -> bool {
    glz_regex().is_match(host)
}

/// True for hosts that may receive Riot auth headers.
pub fn is_riot_api_host(host: &str) -> bool {
    pd_regex().is_match(host) || is_valid_glz_host(host)
}

/// `-ares-deployment=<region>` from VALORANT's launch arguments.
pub fn region_from_launch_args<S: AsRef<str>>(args: &[S]) -> Option<String> {
    args.iter()
        .find_map(|a| a.as_ref().strip_prefix("-ares-deployment="))
        .map(|r| r.to_ascii_lowercase())
}

/// `-config-endpoint=https://shared.<shard>.a.pvp.net` -> shard name.
pub fn shard_from_launch_args<S: AsRef<str>>(args: &[S]) -> Option<String> {
    args.iter().find_map(|a| {
        let url = a.as_ref().strip_prefix("-config-endpoint=")?;
        let host = url.trim_start_matches("https://").split('/').next()?;
        let shard = host.strip_prefix("shared.")?.strip_suffix(".a.pvp.net")?;
        Some(shard.to_ascii_lowercase())
    })
}

/// Region + shard from launch arguments, if VALORANT is running.
pub fn from_launch_args<S: AsRef<str>>(args: &[S]) -> Option<(Region, Shard)> {
    let deployment = region_from_launch_args(args);
    let shard_hint = shard_from_launch_args(args).and_then(|s| Shard::parse(&s));
    let region = deployment
        .as_deref()
        .and_then(Region::from_hint)
        .or_else(|| shard_hint.and_then(|s| Region::from_hint(s.as_str())))?;
    let shard = match (deployment.as_deref(), shard_hint) {
        (Some("pbe"), _) => Shard::Pbe,
        (_, Some(s)) if s.fits(region) => s,
        _ => region.default_shard(),
    };
    Some((region, shard))
}
