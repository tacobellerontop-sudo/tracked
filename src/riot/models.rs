//! Serde structs for Riot payloads (raw, as returned by PD/GLZ) and the
//! normalized models the rest of the app uses.
//!
//! Raw structs are deliberately permissive (`#[serde(default)]` everywhere)
//! because Riot adds and drops fields between patches.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- PD: MMR --

/// `GET /mmr/v1/players/{puuid}/competitiveupdates`
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct CompetitiveUpdates {
    pub subject: String,
    pub matches: Vec<CompetitiveUpdate>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct CompetitiveUpdate {
    #[serde(rename = "MatchID")]
    pub match_id: String,
    #[serde(rename = "MapID")]
    pub map_id: String,
    #[serde(rename = "SeasonID")]
    pub season_id: String,
    pub match_start_time: i64,
    pub tier_after_update: u32,
    pub tier_before_update: u32,
    pub ranked_rating_after_update: i32,
    pub ranked_rating_before_update: i32,
    pub ranked_rating_earned: i32,
    #[serde(rename = "AFKPenalty")]
    pub afk_penalty: i32,
}

/// `GET /mmr/v1/players/{puuid}`
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct PlayerMmr {
    pub subject: String,
    pub queue_skills: HashMap<String, QueueSkill>,
    pub latest_competitive_update: Option<CompetitiveUpdate>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct QueueSkill {
    #[serde(rename = "SeasonalInfoBySeasonID")]
    pub seasonal_info_by_season_id: Option<HashMap<String, SeasonalInfo>>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct SeasonalInfo {
    #[serde(rename = "SeasonID")]
    pub season_id: String,
    pub number_of_wins: u32,
    pub number_of_games: u32,
    pub competitive_tier: u32,
    pub ranked_rating: i32,
    pub wins_by_tier: Option<HashMap<String, u32>>,
}

// ------------------------------------------------------ PD: match history --

/// `GET /match-history/v1/history/{puuid}`
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct MatchHistory {
    pub subject: String,
    pub total: u32,
    pub history: Vec<MatchHistoryEntry>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct MatchHistoryEntry {
    #[serde(rename = "MatchID")]
    pub match_id: String,
    pub game_start_time: i64,
    #[serde(rename = "QueueID")]
    pub queue_id: String,
}

// ------------------------------------------------------ PD: match details --

/// `GET /match-details/v1/matches/{id}` (raw).
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawMatch {
    pub match_info: RawMatchInfo,
    pub players: Vec<RawPlayer>,
    pub teams: Vec<RawTeam>,
    pub round_results: Vec<RawRound>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawMatchInfo {
    pub match_id: String,
    pub map_id: String,
    #[serde(alias = "queueID")]
    pub queue_id: String,
    pub game_mode: String,
    #[serde(alias = "gameLengthMs")]
    pub game_length_millis: u64,
    pub game_start_millis: i64,
    pub is_ranked: bool,
    pub is_completed: bool,
    pub season_id: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawPlayer {
    #[serde(alias = "puuid")]
    pub subject: String,
    pub game_name: String,
    pub tag_line: String,
    pub team_id: String,
    pub party_id: String,
    pub character_id: String,
    pub competitive_tier: u32,
    pub account_level: u32,
    pub stats: Option<RawPlayerStats>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawPlayerStats {
    pub score: u32,
    pub rounds_played: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawTeam {
    pub team_id: String,
    pub won: bool,
    pub rounds_played: u32,
    pub rounds_won: u32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawRound {
    pub round_num: u32,
    pub round_result: String,
    pub round_ceremony: String,
    pub winning_team: String,
    pub winning_team_role: Option<String>,
    pub bomb_planter: Option<String>,
    pub bomb_defuser: Option<String>,
    pub player_stats: Vec<RawRoundPlayer>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawRoundPlayer {
    #[serde(alias = "puuid")]
    pub subject: String,
    pub kills: Vec<RawKill>,
    pub damage: Vec<RawDamage>,
    pub score: i32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawKill {
    pub round_time: u64,
    pub killer: String,
    pub victim: String,
    pub assistants: Vec<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawDamage {
    pub receiver: String,
    pub damage: u32,
    pub legshots: u32,
    pub bodyshots: u32,
    pub headshots: u32,
}

// --------------------------------------------------------- normalized model --

pub type MatchId = String;
pub type Puuid = String;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchInfo {
    pub match_id: MatchId,
    pub map_id: String,
    pub queue_id: String,
    pub game_mode: String,
    pub game_length_ms: u64,
    pub started_at_unix: i64,
    pub is_ranked: bool,
    pub is_completed: bool,
    pub season_id: String,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStats {
    pub score: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub headshots: u32,
    pub bodyshots: u32,
    pub legshots: u32,
    pub damage_made: u32,
    pub damage_taken: u32,
    pub kast_rounds: u32,
    pub first_kills: u32,
    pub first_deaths: u32,
    pub traded_deaths: u32,
    pub multi_kill_rounds3: u32,
    pub multi_kill_rounds4: u32,
    pub aces: u32,
    pub clutches_won: u32,
    pub plants: u32,
    pub defuses: u32,
    pub attack_rounds_won: u32,
    pub defence_rounds_won: u32,
    pub rounds_played: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerMatch {
    pub puuid: Puuid,
    pub game_name: String,
    pub tag_line: String,
    pub team_id: String,
    pub party_id: String,
    pub character_id: String,
    pub competitive_tier: u32,
    pub account_level: u32,
    pub stats: PlayerStats,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamResult {
    pub team_id: String,
    pub won: bool,
    pub rounds_won: u32,
    pub rounds_played: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundResult {
    pub round_num: u32,
    pub winning_team: String,
    pub winning_team_role: Option<String>,
    pub round_ceremony: String,
    pub round_result: String,
    pub player_stats: Vec<RoundPlayerStat>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundPlayerStat {
    pub puuid: Puuid,
    pub kills: u32,
    pub damage: u32,
    pub score: i32,
}

/// A match normalized into our own model, with derived stats filled in.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDetail {
    pub info: MatchInfo,
    pub players: Vec<PlayerMatch>,
    pub teams: Vec<TeamResult>,
    pub rounds: Vec<RoundResult>,
}

impl MatchDetail {
    pub fn player(&self, puuid: &str) -> Option<&PlayerMatch> {
        self.players.iter().find(|p| p.puuid == puuid)
    }

    pub fn team(&self, team_id: &str) -> Option<&TeamResult> {
        self.teams.iter().find(|t| t.team_id == team_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Win,
    Loss,
    Draw,
}

impl MatchDetail {
    /// Result from `puuid`'s perspective: outcome, our rounds, their rounds.
    pub fn outcome_for(&self, puuid: &str) -> Option<(Outcome, u32, u32)> {
        let me = self.player(puuid)?;
        let mine = self.team(&me.team_id)?;
        let theirs = self
            .teams
            .iter()
            .filter(|t| t.team_id != me.team_id)
            .map(|t| t.rounds_won)
            .max()
            .unwrap_or(0);
        let outcome = if mine.won {
            Outcome::Win
        } else if self.teams.iter().any(|t| t.won) {
            Outcome::Loss
        } else {
            Outcome::Draw
        };
        Some((outcome, mine.rounds_won, theirs))
    }
}

// ------------------------------------------------------------- live game --

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivePhase {
    Party,
    Pregame,
    InGame,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePlayer {
    pub puuid: Puuid,
    /// "Red"/"Blue" in game, the ally team id in pre-game, "Party" in menus.
    pub team_id: String,
    pub character_id: String,
    pub locked: bool,
    /// `None` when the player hides their level.
    pub account_level: Option<u32>,
    /// Streamer mode: we do not resolve or show this player's name.
    pub incognito: bool,
    pub is_self: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveMatch {
    pub phase: LivePhase,
    pub match_id: String,
    pub map_id: String,
    pub queue_id: String,
    pub server: String,
    pub players: Vec<LivePlayer>,
}

impl LiveMatch {
    /// Team ids with our own team first.
    pub fn teams(&self) -> Vec<String> {
        let mut teams: Vec<String> = Vec::new();
        if let Some(me) = self.players.iter().find(|p| p.is_self) {
            teams.push(me.team_id.clone());
        }
        for p in &self.players {
            if !teams.contains(&p.team_id) {
                teams.push(p.team_id.clone());
            }
        }
        teams
    }
}
