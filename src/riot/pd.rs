//! Player-data (PD) endpoints: MMR, competitive updates, match history,
//! match details.

use super::match_stats;
use super::models::*;
use super::RiotSession;
use crate::error::ApiError;

pub async fn player_mmr(s: &RiotSession, puuid: &str) -> Result<PlayerMmr, ApiError> {
    s.get(&format!("{}/mmr/v1/players/{puuid}", s.pd()?)).await
}

pub async fn competitive_updates(s: &RiotSession, puuid: &str, count: u32) -> Result<CompetitiveUpdates, ApiError> {
    let url = format!(
        "{}/mmr/v1/players/{puuid}/competitiveupdates?startIndex=0&endIndex={count}&queue=competitive",
        s.pd()?
    );
    s.get(&url).await
}

pub async fn match_history(s: &RiotSession, puuid: &str, start: u32, end: u32) -> Result<MatchHistory, ApiError> {
    let url = format!(
        "{}/match-history/v1/history/{puuid}?startIndex={start}&endIndex={end}",
        s.pd()?
    );
    s.get(&url).await
}

pub async fn match_details(s: &RiotSession, match_id: &str) -> Result<MatchDetail, ApiError> {
    if !is_uuid(match_id) {
        return Err(ApiError::Other("invalid match id".into()));
    }
    let raw: RawMatch = s
        .get(&format!("{}/match-details/v1/matches/{match_id}", s.pd()?))
        .await?;
    Ok(match_stats::normalize(&raw))
}

/// Riot ids (PUUIDs, match ids) are UUIDs; reject anything else before it
/// ends up in a URL path.
pub fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// The rank picture derived from `/mmr/v1/players/{puuid}`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RankSnapshot {
    pub tier: u32,
    pub rr: i32,
    pub season_wins: u32,
    pub season_games: u32,
    pub peak_tier: u32,
}

impl RankSnapshot {
    pub fn win_rate(&self) -> Option<f32> {
        (self.season_games > 0).then(|| 100.0 * self.season_wins as f32 / self.season_games as f32)
    }
}

/// Current-act rank from an MMR payload. If `current_act` is known and the
/// latest update is from an older act, the player is unrated this act.
pub fn rank_snapshot(mmr: &PlayerMmr, current_act: Option<&str>) -> RankSnapshot {
    let seasonal = mmr
        .queue_skills
        .get("competitive")
        .and_then(|q| q.seasonal_info_by_season_id.as_ref());
    let act = current_act
        .map(str::to_string)
        .or_else(|| mmr.latest_competitive_update.as_ref().map(|u| u.season_id.clone()));
    let info = act.as_deref().and_then(|a| seasonal.and_then(|m| m.get(a)));

    let mut snap = RankSnapshot::default();
    if let Some(info) = info {
        snap.tier = info.competitive_tier;
        snap.rr = info.ranked_rating;
        snap.season_wins = info.number_of_wins;
        snap.season_games = info.number_of_games;
    }
    if let Some(latest) = &mmr.latest_competitive_update {
        if act.as_deref() == Some(latest.season_id.as_str()) && latest.tier_after_update > 0 {
            snap.tier = latest.tier_after_update;
            snap.rr = latest.ranked_rating_after_update;
        }
    }
    snap.peak_tier = seasonal
        .into_iter()
        .flat_map(|m| m.values())
        .flat_map(|i| i.wins_by_tier.iter().flat_map(|w| w.keys()))
        .filter_map(|k| k.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        .max(snap.tier);
    snap
}

/// What we show next to a player in a lobby: rank + recent RR swings.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PlayerProbe {
    pub rank: RankSnapshot,
    /// RR earned in the last few competitive games, newest first.
    pub recent_rr: Vec<i32>,
}

impl PlayerProbe {
    pub fn recent_wins(&self) -> (usize, usize) {
        let wins = self.recent_rr.iter().filter(|r| **r > 0).count();
        (wins, self.recent_rr.len())
    }
}

/// Rank-probe one player (two PD calls).
pub async fn probe(s: &RiotSession, puuid: &str, current_act: Option<&str>) -> Result<PlayerProbe, ApiError> {
    if !is_uuid(puuid) {
        return Err(ApiError::Other("invalid player id".into()));
    }
    let (mmr, updates) = tokio::join!(player_mmr(s, puuid), competitive_updates(s, puuid, 5));
    let rank = match mmr {
        Ok(m) => rank_snapshot(&m, current_act),
        Err(ApiError::NotFound) => RankSnapshot::default(),
        Err(e) => return Err(e),
    };
    let recent_rr = updates
        .map(|u| {
            u.matches
                .iter()
                .filter(|m| m.tier_after_update > 0)
                .map(|m| m.ranked_rating_earned)
                .collect()
        })
        .unwrap_or_default();
    Ok(PlayerProbe { rank, recent_rr })
}
