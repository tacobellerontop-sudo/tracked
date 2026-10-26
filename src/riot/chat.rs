//! Built-in social through the Riot Client's own chat service (local API):
//! friends, friend requests, live friend status and whispers.
//!
//! Endpoints and fields per the community valapidocs reference:
//! `GET /chat/v4/friends`, `GET /chat/v4/friendrequests`,
//! `GET /chat/v4/presences`, `GET /chat/v6/conversations`,
//! `GET /chat/v6/messages?cid=…`, `POST /chat/v6/messages`
//! (`{cid, message, type: "chat"}`; a friend's `pid` is the whisper cid).
//!
//! Everything stays between this app and the Riot Client on 127.0.0.1.
//! Message contents are never logged or written to disk.

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::local::LocalClient;
use super::presence::{LoopState, RawPresence};
use crate::error::ApiError;

/// Longest whisper we'll send.
pub const MAX_MESSAGE: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Friend {
    pub puuid: String,
    /// Chat id; also the cid of a whisper conversation with this friend.
    pub pid: String,
    pub game_name: String,
    pub game_tag: String,
    pub note: String,
    pub last_online_ts: Option<i64>,
}

impl Friend {
    pub fn riot_id(&self) -> String {
        if self.game_tag.is_empty() {
            self.game_name.clone()
        } else {
            format!("{}#{}", self.game_name, self.game_tag)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct FriendRequest {
    pub puuid: String,
    pub game_name: String,
    pub game_tag: String,
    /// "pending_in" (they asked you) or "pending_out" (you asked them).
    pub subscription: String,
}

impl FriendRequest {
    pub fn incoming(&self) -> bool {
        self.subscription == "pending_in"
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Conversation {
    pub cid: String,
    /// "chat" (whisper) or "groupchat" (party / team / all chat).
    #[serde(rename = "type")]
    pub kind: String,
    pub unread_count: u32,
    pub muted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatMessage {
    pub id: String,
    pub cid: String,
    pub body: String,
    pub puuid: String,
    pub game_name: String,
    pub game_tag: String,
    /// Milliseconds since the epoch, as a string.
    pub time: String,
    pub read: bool,
    #[serde(rename = "type")]
    pub kind: String,
}

impl ChatMessage {
    pub fn time_ms(&self) -> i64 {
        self.time.parse().unwrap_or(0)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FriendsResp {
    friends: Vec<Friend>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RequestsResp {
    requests: Vec<FriendRequest>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ConversationsResp {
    conversations: Vec<Conversation>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct MessagesResp {
    pub messages: Vec<ChatMessage>,
}

pub async fn friends(local: &LocalClient) -> Result<Vec<Friend>, ApiError> {
    Ok(local.get::<FriendsResp>("/chat/v4/friends").await?.friends)
}

pub async fn friend_requests(local: &LocalClient) -> Result<Vec<FriendRequest>, ApiError> {
    Ok(local.get::<RequestsResp>("/chat/v4/friendrequests").await?.requests)
}

pub async fn conversations(local: &LocalClient) -> Result<Vec<Conversation>, ApiError> {
    Ok(local
        .get::<ConversationsResp>("/chat/v6/conversations")
        .await?
        .conversations)
}

/// Whisper history for one conversation, oldest first.
pub async fn history(local: &LocalClient, cid: &str) -> Result<Vec<ChatMessage>, ApiError> {
    let cid = valid_cid(cid)?;
    let mut msgs = local
        .get::<MessagesResp>(&format!("/chat/v6/messages?cid={cid}"))
        .await?
        .messages;
    msgs.sort_by_key(|m| m.time_ms());
    Ok(msgs)
}

/// Send a whisper. Returns the message(s) as the client recorded them.
pub async fn send(local: &LocalClient, cid: &str, text: &str) -> Result<Vec<ChatMessage>, ApiError> {
    let cid = valid_cid(cid)?;
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_MESSAGE {
        return Err(ApiError::Other(format!("Messages must be 1–{MAX_MESSAGE} characters")));
    }
    let body = serde_json::json!({ "cid": cid, "message": text, "type": "chat" });
    Ok(local
        .post::<_, MessagesResp>("/chat/v6/messages", &body)
        .await?
        .messages)
}

/// A chat id goes into a query string: allow only the characters pids use
/// (`<puuid>@<server>.pvp.net`).
pub fn valid_cid(cid: &str) -> Result<&str, ApiError> {
    let ok = !cid.is_empty()
        && cid.len() <= 128
        && cid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'));
    ok.then_some(cid)
        .ok_or_else(|| ApiError::Other("invalid chat id".into()))
}

/// Extract chat messages from a websocket frame:
/// `[8, "OnJsonApiEvent_chat_v6_messages", {"data": {"messages": [...]}}]`.
pub fn messages_from_event(text: &str) -> Option<Vec<ChatMessage>> {
    let v: Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    if arr.first()?.as_u64()? != 8 || arr.get(1)?.as_str()? != MESSAGES_EVENT {
        return None;
    }
    serde_json::from_value(arr.get(2)?.get("data")?.get("messages")?.clone()).ok()
}

pub const MESSAGES_EVENT: &str = "OnJsonApiEvent_chat_v6_messages";
pub const SUBSCRIBE_MESSAGES: &str = r#"[5,"OnJsonApiEvent_chat_v6_messages"]"#;

/// What a friend is doing, from their presence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Activity {
    #[default]
    Offline,
    /// Signed in to the Riot Client but not in VALORANT.
    RiotClient,
    /// Playing another Riot game (product name).
    OtherGame(String),
    Away,
    Menus,
    InQueue,
    AgentSelect,
    InGame,
}

impl Activity {
    pub fn online(&self) -> bool {
        !matches!(self, Activity::Offline)
    }

    /// In agent select, in a match or in queue.
    pub fn busy(&self) -> bool {
        matches!(self, Activity::InQueue | Activity::AgentSelect | Activity::InGame)
    }

    pub fn label(&self) -> String {
        match self {
            Activity::Offline => "Offline".into(),
            Activity::RiotClient => "Online".into(),
            Activity::OtherGame(p) => match p.as_str() {
                "league_of_legends" => "Playing League of Legends".into(),
                "tft" | "teamfight_tactics" => "Playing TFT".into(),
                "lor" | "bacon" => "Playing Legends of Runeterra".into(),
                other => format!("Playing {other}"),
            },
            Activity::Away => "Away".into(),
            Activity::Menus => "In menus".into(),
            Activity::InQueue => "In queue".into(),
            Activity::AgentSelect => "Agent select".into(),
            Activity::InGame => "In game".into(),
        }
    }
}

/// A friend's live status, decoded from their presence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FriendStatus {
    pub activity: Activity,
    pub queue_id: String,
    pub map: String,
    pub party_size: u32,
    pub competitive_tier: u32,
    pub account_level: u32,
    pub player_card_id: String,
    /// (their team, other team) when the presence carries a score.
    pub score: Option<(u32, u32)>,
}

impl FriendStatus {
    /// Square player-card art for the avatar (valorant-api.com media URL).
    pub fn card_icon(&self) -> Option<String> {
        super::pd::is_uuid(&self.player_card_id).then(|| {
            format!(
                "https://media.valorant-api.com/playercards/{}/displayicon.png",
                self.player_card_id
            )
        })
    }
}

/// Decode a friend's presence. Handles both private-blob layouts (flat, and
/// nested `matchPresenceData` / `partyPresenceData` / `playerPresenceData`).
pub fn friend_status(p: &RawPresence) -> FriendStatus {
    let mut s = FriendStatus::default();
    if p.product != "valorant" {
        s.activity = match p.product.as_str() {
            "" | "riot_client" | "keystone" => Activity::RiotClient,
            other => Activity::OtherGame(other.into()),
        };
        if p.state == "away" {
            s.activity = Activity::Away;
        }
        return s;
    }
    let blob = p
        .private
        .as_deref()
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b.trim()).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);
    let m = blob.get("matchPresenceData").unwrap_or(&blob);
    let party = blob.get("partyPresenceData").unwrap_or(&blob);
    let player = blob.get("playerPresenceData").unwrap_or(&blob);
    let st = |o: &Value, k: &str| o.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let num = |o: &Value, k: &str| o.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;

    let loop_state = LoopState::parse(&st(m, "sessionLoopState"));
    let party_state = st(party, "partyState");
    s.activity = match loop_state {
        LoopState::InGame => Activity::InGame,
        LoopState::Pregame => Activity::AgentSelect,
        _ if party_state == "MATCHMAKING" => Activity::InQueue,
        _ if p.state == "away" => Activity::Away,
        _ => Activity::Menus,
    };
    s.queue_id = st(m, "queueId");
    s.map = st(m, "matchMap");
    s.party_size = num(party, "partySize");
    s.competitive_tier = num(player, "competitiveTier");
    s.account_level = num(player, "accountLevel");
    s.player_card_id = st(player, "playerCardId");
    let (ally, enemy) = (
        num(&blob, "partyOwnerMatchScoreAllyTeam"),
        num(&blob, "partyOwnerMatchScoreEnemyTeam"),
    );
    if s.activity == Activity::InGame && (ally > 0 || enemy > 0) {
        s.score = Some((ally, enemy));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presence(product: &str, state: &str, blob: Value) -> RawPresence {
        RawPresence {
            puuid: "p".into(),
            product: product.into(),
            state: state.into(),
            private: Some(base64::engine::general_purpose::STANDARD.encode(blob.to_string())),
            ..Default::default()
        }
    }

    #[test]
    fn decodes_nested_layout() {
        let s = friend_status(&presence(
            "valorant",
            "chat",
            serde_json::json!({
                "matchPresenceData": { "sessionLoopState": "INGAME", "queueId": "competitive", "matchMap": "/Game/Maps/Ascent/Ascent" },
                "partyPresenceData": { "partySize": 3, "partyState": "DEFAULT" },
                "playerPresenceData": { "competitiveTier": 14, "accountLevel": 120, "playerCardId": "9fb348bc-41a0-91ad-8a3e-818035c4e561" }
            }),
        ));
        assert_eq!(s.activity, Activity::InGame);
        assert_eq!((s.party_size, s.competitive_tier, s.account_level), (3, 14, 120));
        assert_eq!(s.queue_id, "competitive");
        assert!(s.card_icon().unwrap().ends_with("/displayicon.png"));
    }

    #[test]
    fn decodes_flat_layout_with_queue_and_score() {
        let queue = friend_status(&presence(
            "valorant",
            "chat",
            serde_json::json!({ "sessionLoopState": "MENUS", "partyState": "MATCHMAKING", "partySize": 2 }),
        ));
        assert_eq!(queue.activity, Activity::InQueue);
        let game = friend_status(&presence(
            "valorant",
            "chat",
            serde_json::json!({ "sessionLoopState": "INGAME", "partyOwnerMatchScoreAllyTeam": 8, "partyOwnerMatchScoreEnemyTeam": 5 }),
        ));
        assert_eq!(game.score, Some((8, 5)));
    }

    #[test]
    fn other_products_and_away() {
        let lol = RawPresence {
            product: "league_of_legends".into(),
            ..Default::default()
        };
        assert_eq!(friend_status(&lol).activity.label(), "Playing League of Legends");
        let away = friend_status(&presence(
            "valorant",
            "away",
            serde_json::json!({ "sessionLoopState": "MENUS" }),
        ));
        assert_eq!(away.activity, Activity::Away);
    }

    #[test]
    fn message_frames_and_cids() {
        let frame = r#"[8,"OnJsonApiEvent_chat_v6_messages",{"data":{"messages":[{"id":"1","cid":"abc@eu1.pvp.net","body":"gg","puuid":"x","time":"1700000000000","type":"chat"}]},"eventType":"Create"}]"#;
        let m = messages_from_event(frame).unwrap();
        assert_eq!(m[0].body, "gg");
        assert_eq!(m[0].time_ms(), 1_700_000_000_000);
        assert!(messages_from_event(r#"[8,"OnJsonApiEvent_chat_v4_presences",{"data":{}}]"#).is_none());
        assert!(valid_cid("0b0e4a2c-1111-4b6c-9c6e-123456789abc@eu1.pvp.net").is_ok());
        assert!(valid_cid("x&cid=y").is_err());
        assert!(valid_cid("").is_err());
    }
}
