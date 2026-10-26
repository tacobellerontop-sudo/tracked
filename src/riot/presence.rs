//! Presence: decoding the private blob and the local websocket feed.

use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use super::tls;
use crate::secret::Secret;

/// One entry of the local chat presence list.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct RawPresence {
    pub puuid: String,
    pub product: String,
    pub game_name: String,
    pub game_tag: String,
    /// Base64-encoded JSON blob. For VALORANT it carries the session loop state.
    pub private: Option<String>,
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

/// Events emitted by the websocket task.
#[derive(Debug, Clone)]
pub enum PresenceEvent {
    Connected,
    Changed(GamePresence),
    Disconnected,
}

/// Connect to the local websocket and forward our presence changes until
/// the socket drops. The caller reconnects with backoff.
pub async fn run_socket(port: u16, password: Secret<String>, puuid: String, tx: mpsc::UnboundedSender<PresenceEvent>) {
    let result = socket_once(port, &password, &puuid, &tx).await;
    if let Err(e) = result {
        tracing::debug!("presence socket ended: {e}");
    }
    let _ = tx.send(PresenceEvent::Disconnected);
}

async fn socket_once(
    port: u16,
    password: &Secret<String>,
    puuid: &str,
    tx: &mpsc::UnboundedSender<PresenceEvent>,
) -> Result<(), String> {
    // Credentials travel in the Authorization header, never in the URL.
    let mut req = format!("wss://127.0.0.1:{port}")
        .into_client_request()
        .map_err(|e| e.to_string())?;
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("riot:{}", password.expose()));
    let mut value = format!("Basic {basic}")
        .parse::<tokio_tungstenite::tungstenite::http::HeaderValue>()
        .map_err(|e| e.to_string())?;
    value.set_sensitive(true);
    req.headers_mut().insert("Authorization", value);

    let connector = tokio_tungstenite::Connector::Rustls(tls::local_client_config());
    let (mut ws, _) = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async_tls_with_config(req, None, false, Some(connector)),
    )
    .await
    .map_err(|_| "connect timeout".to_string())?
    .map_err(|e| e.to_string())?;

    ws.send(Message::Text(SUBSCRIBE_PRESENCES.into()))
        .await
        .map_err(|e| e.to_string())?;
    let _ = tx.send(PresenceEvent::Connected);
    tracing::info!("presence websocket connected");

    let mut last: Option<GamePresence> = None;
    while let Some(msg) = ws.next().await {
        let msg = msg.map_err(|e| e.to_string())?;
        let Message::Text(text) = msg else { continue };
        if text.is_empty() {
            continue;
        }
        let Some(list) = presences_from_event(&text) else {
            continue;
        };
        if let Some(gp) = own_presence(&list, puuid) {
            if last.as_ref() != Some(&gp) {
                last = Some(gp.clone());
                if tx.send(PresenceEvent::Changed(gp)).is_err() {
                    break;
                }
            }
        }
    }
    Ok(())
}
