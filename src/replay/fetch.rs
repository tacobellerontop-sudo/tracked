//! `GET pd.<shard>/match-details/v1/matches/<id>` and the slice of it the
//! replay needs. Field names follow valapidocs ("GET MatchDetails_FetchMatchDetails"):
//! kills use `roundTime`/`gameTime` and positions are keyed by `subject`.
//! The `timeSince…`/`puuid` names used by Riot's official match-v1 API are
//! accepted as aliases.

use serde::Deserialize;

use super::Loc;
use crate::error::ApiError;
use crate::riot::{pd, RiotSession};

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MatchDetailsRaw {
    pub match_info: MatchInfoRaw,
    pub players: Vec<PlayerRaw>,
    pub teams: Vec<TeamRaw>,
    pub round_results: Vec<RoundRaw>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MatchInfoRaw {
    pub match_id: String,
    pub map_id: String,
    #[serde(alias = "queueID")]
    pub queue_id: String,
    pub game_mode: String,
    pub game_start_millis: i64,
    pub is_ranked: bool,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlayerRaw {
    #[serde(alias = "puuid")]
    pub subject: String,
    pub game_name: String,
    pub tag_line: String,
    pub team_id: String,
    pub character_id: String,
    pub competitive_tier: u32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TeamRaw {
    pub team_id: String,
    pub won: bool,
    pub rounds_won: u32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RoundRaw {
    pub round_num: u32,
    pub round_result: String,
    pub winning_team: String,
    pub bomb_planter: Option<String>,
    /// 0 when the spike wasn't planted.
    pub plant_round_time: Option<u32>,
    pub plant_location: Option<Loc>,
    pub plant_site: Option<String>,
    pub plant_player_locations: Option<Vec<PlayerLocRaw>>,
    pub bomb_defuser: Option<String>,
    /// 0 when the spike wasn't defused.
    pub defuse_round_time: Option<u32>,
    pub defuse_location: Option<Loc>,
    pub defuse_player_locations: Option<Vec<PlayerLocRaw>>,
    pub player_stats: Vec<RoundPlayerRaw>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RoundPlayerRaw {
    #[serde(alias = "puuid")]
    pub subject: String,
    pub kills: Vec<KillRaw>,
    pub damage: Vec<DamageRaw>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct KillRaw {
    #[serde(alias = "timeSinceRoundStartMillis")]
    pub round_time: u32,
    #[serde(alias = "timeSinceGameStartMillis")]
    pub game_time: u64,
    pub killer: String,
    pub victim: String,
    pub assistants: Vec<String>,
    pub victim_location: Option<Loc>,
    pub player_locations: Vec<PlayerLocRaw>,
    pub finishing_damage: FinishingDamage,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlayerLocRaw {
    #[serde(alias = "puuid")]
    pub subject: String,
    pub view_radians: f32,
    pub location: Loc,
}

#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FinishingDamage {
    /// "Weapon" | "Ability" | "Bomb" | "Fall" | "Melee" | "".
    pub damage_type: String,
    /// Weapon UUID, or "Ultimate" / "Ability1" / "Ability2" / "GrenadeAbility" / "Primary" / "".
    pub damage_item: String,
    pub is_secondary_fire_mode: bool,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DamageRaw {
    pub receiver: String,
    pub legshots: u32,
    pub bodyshots: u32,
    pub headshots: u32,
}

/// Fetch and parse one match. The id is validated as a UUID before it goes
/// into the URL; the request carries the usual four Riot headers.
pub async fn fetch(s: &RiotSession, match_id: &str) -> Result<MatchDetailsRaw, ApiError> {
    Ok(serde_json::from_value(fetch_json(s, match_id).await?)?)
}

/// The raw JSON, for the on-disk cache (finished matches never change).
pub async fn fetch_json(s: &RiotSession, match_id: &str) -> Result<serde_json::Value, ApiError> {
    if !pd::is_uuid(match_id) {
        return Err(ApiError::Other("invalid match id".into()));
    }
    s.get(&format!("{}/match-details/v1/matches/{match_id}", s.pd()?)).await
}
