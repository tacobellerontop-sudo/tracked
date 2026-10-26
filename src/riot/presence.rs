//! Presence payloads: decoding the base64 `private` blob of a VALORANT
//! presence. The websocket that delivers them lives in [`super::state`].

use base64::Engine;
use serde::Deserialize;
use serde_json::Value;

/// One entry of the local chat presence list.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct RawPresence {
    pub puuid: String,
    pub product: String,
    pub game_name: String,
    pub game_tag: String,
    /// Base64-encoded JSON blob. For VALORANT it carries the session loop state.
    pub private: Option<String>,
    /// "chat", "away", "dnd", "mobile", "offline"…
    pub state: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopState {
    #[default]
    Unknown,
    Menus,
    Pregame,
    InGame,
}

impl LoopState {
    pub fn parse(s: &str) -> Self {
        match s {
            "MENUS" => Self::Menus,
            "PREGAME" => Self::Pregame,
            "INGAME" => Self::InGame,
            _ => Self::Unknown,
        }
    }

    pub fn is_live(self) -> bool {
        matches!(self, Self::Pregame | Self::InGame)
    }
}

/// The interesting parts of the private presence blob.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GamePresence {
    pub loop_state: LoopState,
    pub party_id: String,
    pub party_size: u32,
    pub queue_id: String,
    pub map: String,
    /// "DEFAULT", "MATCHMAKING" (in queue), "MATCHMADE_GAME_STARTING"…
    pub party_state: String,
}

impl GamePresence {
    pub fn in_queue(&self) -> bool {
        self.loop_state == LoopState::Menus && self.party_state == "MATCHMAKING"
    }
}

/// Decode the base64 private blob. Handles both the flat (older) layout and
/// the nested `matchPresenceData`/`partyPresenceData` layout.
pub fn decode_private(b64: &str) -> Option<GamePresence> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    let m = v.get("matchPresenceData").unwrap_or(&v);
    let p = v.get("partyPresenceData").unwrap_or(&v);
    let s = |obj: &Value, k: &str| obj.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let loop_state = m.get("sessionLoopState").and_then(Value::as_str)?;
    Some(GamePresence {
        loop_state: LoopState::parse(loop_state),
        party_id: s(p, "partyId"),
        party_size: p.get("partySize").and_then(Value::as_u64).unwrap_or(0) as u32,
        queue_id: s(m, "queueId"),
        map: s(m, "matchMap"),
        party_state: s(p, "partyState"),
    })
}

/// Pick our own VALORANT presence out of a list and decode it.
pub fn own_presence(presences: &[RawPresence], puuid: &str) -> Option<GamePresence> {
    presences
        .iter()
        .filter(|p| p.puuid == puuid && p.product == "valorant")
        .find_map(|p| p.private.as_deref().and_then(decode_private))
}

/// Extract presences from a websocket event frame:
/// `[8, "OnJsonApiEvent_chat_v4_presences", {"data": {"presences": [...]}}]`.
pub fn presences_from_event(text: &str) -> Option<Vec<RawPresence>> {
    let v: Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    if arr.first()?.as_u64()? != 8 {
        return None;
    }
    let list = arr.get(2)?.get("data")?.get("presences")?;
    serde_json::from_value(list.clone()).ok()
}

pub const SUBSCRIBE_PRESENCES: &str = r#"[5,"OnJsonApiEvent_chat_v4_presences"]"#;
