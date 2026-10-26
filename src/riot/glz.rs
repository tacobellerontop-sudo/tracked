//! Game-lifecycle (GLZ) endpoints: party, pre-game (agent select) and
//! core-game (in match). A 404 simply means "not in that state".

use serde::Deserialize;

use super::models::{LiveMatch, LivePhase, LivePlayer};
use super::RiotSession;
use crate::error::ApiError;

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct PlayerIdentity {
    pub account_level: u32,
    pub incognito: bool,
    pub hide_account_level: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MatchRef {
    #[serde(rename = "MatchID")]
    match_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PartyRef {
    #[serde(rename = "CurrentPartyID")]
    current_party_id: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct RawPregame {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "MapID")]
    pub map_id: String,
    #[serde(rename = "QueueID")]
    pub queue_id: String,
    pub ally_team: Option<RawPregameTeam>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct RawPregameTeam {
    #[serde(rename = "TeamID")]
    pub team_id: String,
    pub players: Vec<RawPregamePlayer>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct RawPregamePlayer {
    pub subject: String,
    #[serde(rename = "CharacterID")]
    pub character_id: String,
    /// "", "selected" or "locked".
    pub character_selection_state: String,
    pub player_identity: PlayerIdentity,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct RawCoregame {
    #[serde(rename = "MatchID")]
    pub match_id: String,
    #[serde(rename = "MapID")]
    pub map_id: String,
    #[serde(rename = "GamePodID")]
    pub game_pod_id: String,
    pub matchmaking_data: Option<RawMatchmaking>,
    pub players: Vec<RawCoregamePlayer>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct RawMatchmaking {
    #[serde(rename = "QueueID")]
    pub queue_id: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct RawCoregamePlayer {
    pub subject: String,
    #[serde(rename = "TeamID")]
    pub team_id: String,
    #[serde(rename = "CharacterID")]
    pub character_id: String,
    pub player_identity: PlayerIdentity,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct RawParty {
    #[serde(rename = "ID")]
    pub id: String,
    pub members: Vec<RawPartyMember>,
    pub matchmaking_data: Option<RawMatchmaking>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct RawPartyMember {
    pub subject: String,
    pub is_owner: Option<bool>,
    pub player_identity: PlayerIdentity,
}

/// Turn "not found" into `None`.
async fn optional<T>(fut: impl std::future::Future<Output = Result<T, ApiError>>) -> Result<Option<T>, ApiError> {
    match fut.await {
        Ok(v) => Ok(Some(v)),
        Err(ApiError::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}

fn level(id: &PlayerIdentity) -> Option<u32> {
    (!id.hide_account_level).then_some(id.account_level)
}

/// `GET /pregame/v1/players/{puuid}` → the agent-select match id, if any.
pub async fn pregame_match_id(s: &RiotSession) -> Option<String> {
    let base = s.glz().ok()?;
    let r: MatchRef = s.get(&format!("{base}/pregame/v1/players/{}", s.puuid)).await.ok()?;
    super::pd::is_uuid(&r.match_id).then_some(r.match_id)
}

/// `GET /core-game/v1/players/{puuid}` → the in-progress match id, if any.
pub async fn coregame_match_id(s: &RiotSession) -> Option<String> {
    let base = s.glz().ok()?;
    let r: MatchRef = s.get(&format!("{base}/core-game/v1/players/{}", s.puuid)).await.ok()?;
    super::pd::is_uuid(&r.match_id).then_some(r.match_id)
}

pub async fn pregame(s: &RiotSession) -> Result<Option<LiveMatch>, ApiError> {
    let base = s.glz()?;
    let Some(r) = optional(s.get::<MatchRef>(&format!("{base}/pregame/v1/players/{}", s.puuid))).await? else {
        return Ok(None);
    };
    if !super::pd::is_uuid(&r.match_id) {
        return Ok(None);
    }
    let m: RawPregame = s.get(&format!("{base}/pregame/v1/matches/{}", r.match_id)).await?;
    Ok(Some(pregame_to_live(&m, &s.puuid)))
}

pub fn pregame_to_live(m: &RawPregame, me: &str) -> LiveMatch {
    let team = m.ally_team.clone().unwrap_or_default();
    LiveMatch {
        phase: LivePhase::Pregame,
        match_id: m.id.clone(),
        map_id: m.map_id.clone(),
        queue_id: m.queue_id.clone(),
        server: String::new(),
        players: team
            .players
            .iter()
            .map(|p| LivePlayer {
                puuid: p.subject.clone(),
                team_id: team.team_id.clone(),
                character_id: p.character_id.to_ascii_lowercase(),
                locked: p.character_selection_state == "locked",
                account_level: level(&p.player_identity),
                incognito: p.player_identity.incognito && p.subject != me,
                is_self: p.subject == me,
            })
            .collect(),
    }
}

pub async fn coregame(s: &RiotSession) -> Result<Option<LiveMatch>, ApiError> {
    let base = s.glz()?;
    let Some(r) = optional(s.get::<MatchRef>(&format!("{base}/core-game/v1/players/{}", s.puuid))).await? else {
        return Ok(None);
    };
    if !super::pd::is_uuid(&r.match_id) {
        return Ok(None);
    }
    let m: RawCoregame = s.get(&format!("{base}/core-game/v1/matches/{}", r.match_id)).await?;
    Ok(Some(coregame_to_live(&m, &s.puuid)))
}

pub fn coregame_to_live(m: &RawCoregame, me: &str) -> LiveMatch {
    LiveMatch {
        phase: LivePhase::InGame,
        match_id: m.match_id.clone(),
        map_id: m.map_id.clone(),
        queue_id: m
            .matchmaking_data
            .as_ref()
            .map(|d| d.queue_id.clone())
            .unwrap_or_default(),
        server: m.game_pod_id.clone(),
        players: m
            .players
            .iter()
            .map(|p| LivePlayer {
                puuid: p.subject.clone(),
                team_id: p.team_id.clone(),
                character_id: p.character_id.to_ascii_lowercase(),
                locked: true,
                account_level: level(&p.player_identity),
                incognito: p.player_identity.incognito && p.subject != me,
                is_self: p.subject == me,
            })
            .collect(),
    }
}

pub async fn party(s: &RiotSession) -> Result<Option<LiveMatch>, ApiError> {
    let base = s.glz()?;
    let Some(r) = optional(s.get::<PartyRef>(&format!("{base}/parties/v1/players/{}", s.puuid))).await? else {
        return Ok(None);
    };
    if !super::pd::is_uuid(&r.current_party_id) {
        return Ok(None);
    }
    let p: RawParty = s
        .get(&format!("{base}/parties/v1/parties/{}", r.current_party_id))
        .await?;
    Ok(Some(party_to_live(&p, &s.puuid)))
}

pub fn party_to_live(p: &RawParty, me: &str) -> LiveMatch {
    LiveMatch {
        phase: LivePhase::Party,
        match_id: p.id.clone(),
        map_id: String::new(),
        queue_id: p
            .matchmaking_data
            .as_ref()
            .map(|d| d.queue_id.clone())
            .unwrap_or_default(),
        server: String::new(),
        players: p
            .members
            .iter()
            .map(|m| LivePlayer {
                puuid: m.subject.clone(),
                team_id: "Party".into(),
                character_id: String::new(),
                locked: false,
                account_level: level(&m.player_identity),
                // Party members are people you queued with; still honour their setting.
                incognito: m.player_identity.incognito && m.subject != me,
                is_self: m.subject == me,
            })
            .collect(),
    }
}

/// The most specific live state we can find, preferring the one presence
/// says we're in.
pub async fn current(s: &RiotSession, hint: super::presence::LoopState) -> Result<Option<LiveMatch>, ApiError> {
    use super::presence::LoopState::*;
    if hint == InGame {
        if let Some(m) = coregame(s).await? {
            return Ok(Some(m));
        }
    }
    if matches!(hint, Pregame | InGame) {
        if let Some(m) = pregame(s).await? {
            return Ok(Some(m));
        }
    }
    if hint == Unknown {
        if let Some(m) = coregame(s).await? {
            return Ok(Some(m));
        }
        if let Some(m) = pregame(s).await? {
            return Ok(Some(m));
        }
    }
    party(s).await
}
