//! The social websocket protocol.
//!
//! Client -> server: `subscribe`, `heartbeat`, `presence_lookup`,
//! `presence_usage`, `lobby_push`, `lobby_touch`.
//! Server -> client: `hello`, `heartbeat_ack`, `subscribed`, `presence`,
//! `lobby`, `lobby_touched`, `dm`, `typing`, `conversation`, `error`.
//! Every frame is a JSON object with a `type` field.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use super::models::{Conversation, DmMessage, LobbyInfo, PresenceEntry};
use crate::secret::Secret;

pub const HEARTBEAT: Duration = Duration::from_secs(25);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Subscribe {
        topics: Vec<String>,
    },
    Heartbeat,
    PresenceLookup {
        user_ids: Vec<String>,
    },
    PresenceUsage {
        state: String,
        queue: String,
        party_size: u32,
    },
    LobbyPush {
        lobby: Option<LobbyInfo>,
    },
    LobbyTouch,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Hello {
        user_id: String,
    },
    HeartbeatAck {},
    Subscribed {},
    Presence {
        entries: Vec<PresenceEntry>,
    },
    Lobby {
        user_id: String,
        lobby: Option<LobbyInfo>,
    },
    LobbyTouched {},
    Dm {
        conversation_id: String,
        message: DmMessage,
    },
    Typing {
        conversation_id: String,
        user_id: String,
    },
    Conversation {
        conversation: Conversation,
    },
    Error {
        message: String,
    },
    #[serde(other)]
    Unknown,
}

pub fn parse(text: &str) -> Option<ServerMsg> {
    serde_json::from_str(text).ok()
}

#[derive(Debug)]
pub enum SocketEvent {
    Connected,
    Message(ServerMsg),
    Closed(String),
}

fn tls_connector() -> tokio_tungstenite::Connector {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring supports default versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    tokio_tungstenite::Connector::Rustls(Arc::new(config))
}

/// Run one websocket session until it closes. Outgoing messages come from
/// `out`; everything received is forwarded to `events`.
pub async fn run(
    url: reqwest::Url,
    token: Secret<String>,
    topics: Vec<String>,
    mut out: mpsc::UnboundedReceiver<ClientMsg>,
    events: mpsc::UnboundedSender<SocketEvent>,
) {
    let reason = match session(url, token, topics, &mut out, &events).await {
        Ok(()) => "closed".to_string(),
        Err(e) => e,
    };
    let _ = events.send(SocketEvent::Closed(reason));
}

async fn session(
    url: reqwest::Url,
    token: Secret<String>,
    topics: Vec<String>,
    out: &mut mpsc::UnboundedReceiver<ClientMsg>,
    events: &mpsc::UnboundedSender<SocketEvent>,
) -> Result<(), String> {
    let mut req = url.as_str().into_client_request().map_err(|e| e.to_string())?;
    let mut auth = format!("Bearer {}", token.expose())
        .parse::<tokio_tungstenite::tungstenite::http::HeaderValue>()
        .map_err(|e| e.to_string())?;
    auth.set_sensitive(true);
    req.headers_mut().insert("Authorization", auth);
    let connector = (url.scheme() == "wss").then(tls_connector);
    let (ws, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async_tls_with_config(req, None, false, connector),
    )
    .await
    .map_err(|_| "connect timeout".to_string())?
    .map_err(|e| e.to_string())?;
    let (mut sink, mut stream) = ws.split();

    let send = |m: &ClientMsg| Message::Text(serde_json::to_string(m).expect("serializable"));
    sink.send(send(&ClientMsg::Subscribe { topics }))
        .await
        .map_err(|e| e.to_string())?;
    let _ = events.send(SocketEvent::Connected);

    let mut beat = tokio::time::interval(HEARTBEAT);
    beat.tick().await;
    loop {
        tokio::select! {
            msg = out.recv() => match msg {
                Some(m) => sink.send(send(&m)).await.map_err(|e| e.to_string())?,
                None => return Ok(()), // owner went away
            },
            _ = beat.tick() => sink.send(send(&ClientMsg::Heartbeat)).await.map_err(|e| e.to_string())?,
            frame = stream.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    if let Some(m) = parse(&text) {
                        let _ = events.send(SocketEvent::Message(m));
                    }
                }
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.to_string()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_messages_use_the_wire_names() {
        let j = |m: ClientMsg| serde_json::to_value(m).unwrap();
        assert_eq!(j(ClientMsg::Heartbeat), serde_json::json!({ "type": "heartbeat" }));
        assert_eq!(j(ClientMsg::LobbyTouch), serde_json::json!({ "type": "lobby_touch" }));
        assert_eq!(
            j(ClientMsg::PresenceLookup {
                user_ids: vec!["a".into()]
            }),
            serde_json::json!({ "type": "presence_lookup", "user_ids": ["a"] })
        );
        assert_eq!(
            j(ClientMsg::PresenceUsage {
                state: "INGAME".into(),
                queue: "competitive".into(),
                party_size: 2
            }),
            serde_json::json!({ "type": "presence_usage", "state": "INGAME", "queue": "competitive", "party_size": 2 })
        );
        assert_eq!(
            j(ClientMsg::LobbyPush { lobby: None }),
            serde_json::json!({ "type": "lobby_push", "lobby": null })
        );
        assert_eq!(
            j(ClientMsg::Subscribe {
                topics: vec!["dm".into()]
            }),
            serde_json::json!({ "type": "subscribe", "topics": ["dm"] })
        );
    }

    #[test]
    fn server_messages_parse() {
        assert_eq!(
            parse(r#"{"type":"heartbeat_ack","server_time":1}"#),
            Some(ServerMsg::HeartbeatAck {})
        );
        assert_eq!(parse(r#"{"type":"something_new","x":1}"#), Some(ServerMsg::Unknown));
        assert!(matches!(
            parse(r#"{"type":"typing","conversation_id":"c","user_id":"u"}"#),
            Some(ServerMsg::Typing { .. })
        ));
        assert!(matches!(
            parse(r#"{"type":"presence","entries":[{"user_id":"u","online":true,"state":"INGAME"}]}"#),
            Some(ServerMsg::Presence { entries }) if entries[0].state.as_deref() == Some("INGAME")
        ));
        assert_eq!(parse("not json"), None);
    }
}
