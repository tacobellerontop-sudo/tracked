//! Live game state (brief §7): the local presence websocket, with a polling
//! fallback and reconnect backoff.
//!
//! The socket authenticates with the same basic credentials as the REST
//! client, sent in an `Authorization` header (not as `riot:<pw>@` in the URL,
//! where URLs tend to end up in logs). TLS uses the same loopback-only setup
//! as [`super::local`].

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, Stream, StreamExt};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use super::presence::{self, GamePresence, LoopState};
use super::{glz, RiotSession};
use crate::error::ApiError;

/// Real-time game state, pushed as it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameState {
    /// Client not running (or VALORANT isn't reporting a presence).
    Offline,
    Menus,
    PreGame {
        match_id: String,
    },
    InGame {
        match_id: String,
    },
}

impl GameState {
    pub fn loop_state(&self) -> LoopState {
        match self {
            GameState::Offline => LoopState::Unknown,
            GameState::Menus => LoopState::Menus,
            GameState::PreGame { .. } => LoopState::Pregame,
            GameState::InGame { .. } => LoopState::InGame,
        }
    }
}

/// Everything the watcher reports. `watch_state` keeps only the states.
#[derive(Debug, Clone, PartialEq)]
pub enum WatchEvent {
    /// The event socket connected.
    SocketUp,
    /// The socket dropped (reason); we're polling until it reconnects.
    SocketDown(String),
    /// State or presence details (party size, queue, map) changed.
    State {
        state: GameState,
        presence: Option<GamePresence>,
    },
    /// Presences of other players (friends): a full snapshot when polled,
    /// only the changed entries when they arrive over the socket.
    Presences(Vec<presence::RawPresence>),
    /// New chat messages (whispers and group chats).
    Chat(Vec<super::chat::ChatMessage>),
}

/// Polling cadence while the socket is down.
pub const POLL_EVERY: Duration = Duration::from_secs(3);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// A de-duplicated stream of [`GameState`]s.
pub fn watch_state(session: Arc<RiotSession>) -> impl Stream<Item = GameState> {
    let mut last: Option<GameState> = None;
    watch(session).filter_map(move |ev| {
        let out = match ev {
            WatchEvent::State { state, .. } if last.as_ref() != Some(&state) => {
                last = Some(state.clone());
                Some(state)
            }
            _ => None,
        };
        std::future::ready(out)
    })
}

/// The full event stream (socket status + state/presence changes). Runs
/// until the stream is dropped; socket errors never end it.
pub fn watch(session: Arc<RiotSession>) -> impl Stream<Item = WatchEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(run(session, tx));
    futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|ev| (ev, rx)) })
}

/// Tracks the last presence and turns presence snapshots into events.
struct Tracker {
    session: Arc<RiotSession>,
    tx: mpsc::UnboundedSender<WatchEvent>,
    presence: Option<GamePresence>,
    state: Option<GameState>,
}

impl Tracker {
    fn send(&self, ev: WatchEvent) -> bool {
        self.tx.send(ev).is_ok()
    }

    /// Feed one presence snapshot (None = no VALORANT presence for us).
    async fn feed(&mut self, presence: Option<GamePresence>) -> bool {
        if presence == self.presence && self.state.is_some() {
            return true;
        }
        let prev_loop = self.presence.as_ref().map(|p| p.loop_state);
        let new_loop = presence.as_ref().map(|p| p.loop_state);
        let state = match (&presence, &self.state) {
            // Same loop state: keep the match id we already resolved.
            (Some(_), Some(s)) if prev_loop == new_loop => s.clone(),
            (Some(p), _) => self.resolve(p.loop_state).await,
            (None, _) => GameState::Offline,
        };
        self.presence = presence.clone();
        self.state = Some(state.clone());
        self.send(WatchEvent::State { state, presence })
    }

    /// Map a loop state to a [`GameState`], looking the match id up on GLZ.
    async fn resolve(&self, loop_state: LoopState) -> GameState {
        let s = &self.session;
        match loop_state {
            LoopState::Pregame => GameState::PreGame {
                match_id: glz::pregame_match_id(s).await.unwrap_or_default(),
            },
            LoopState::InGame => GameState::InGame {
                match_id: glz::coregame_match_id(s).await.unwrap_or_default(),
            },
            LoopState::Menus | LoopState::Unknown => GameState::Menus,
        }
    }
}

async fn run(session: Arc<RiotSession>, tx: mpsc::UnboundedSender<WatchEvent>) {
    let mut t = Tracker {
        session,
        tx,
        presence: None,
        state: None,
    };
    let mut backoff = Duration::from_secs(1);
    loop {
        // Try the socket; it returns when the connection ends.
        match socket(&mut t).await {
            Ok(()) if t.tx.is_closed() => return,
            Ok(()) => {
                backoff = Duration::from_secs(1);
                if !t.send(WatchEvent::SocketDown("closed by the client".into())) {
                    return;
                }
            }
            Err(why) => {
                if !t.send(WatchEvent::SocketDown(why)) {
                    return;
                }
            }
        }
        // Poll until it's time to try the socket again.
        let until = Instant::now() + backoff;
        backoff = (backoff * 2).min(MAX_BACKOFF);
        loop {
            if !poll_once(&mut t).await {
                return;
            }
            if Instant::now() >= until {
                break;
            }
            tokio::time::sleep(POLL_EVERY.min(until - Instant::now())).await;
        }
    }
}

/// One `GET /chat/v4/presences`. Returns false when the stream was dropped.
async fn poll_once(t: &mut Tracker) -> bool {
    match t.session.local.presences().await {
        Ok(p) => {
            let gp = presence::own_presence(&p.presences, &t.session.puuid);
            let others: Vec<_> = p
                .presences
                .iter()
                .filter(|x| x.puuid != t.session.puuid)
                .cloned()
                .collect();
            if !t.send(WatchEvent::Presences(others)) {
                return false;
            }
            t.feed(gp).await
        }
        // Client unreachable: report offline (the supervisor tears down).
        Err(ApiError::Network(_)) => t.feed(None).await,
        Err(e) => {
            tracing::debug!("presence poll failed: {e}");
            !t.tx.is_closed()
        }
    }
}

async fn socket(t: &mut Tracker) -> Result<(), String> {
    let local = &t.session.local;
    let mut req = format!("wss://127.0.0.1:{}", local.port())
        .into_client_request()
        .map_err(|e| e.to_string())?;
    req.headers_mut().insert("Authorization", local.auth().header());
    let (mut ws, _) = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async_tls_with_config(req, None, false, Some(local.ws_connector())),
    )
    .await
    .map_err(|_| "connect timeout".to_string())?
    .map_err(|e| crate::error::chain(&e))?;
    ws.send(Message::Text(super::chat::SUBSCRIBE_MESSAGES.into()))
        .await
        .map_err(|e| e.to_string())?;
    ws.send(Message::Text(presence::SUBSCRIBE_PRESENCES.into()))
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!("presence websocket connected");
    if !t.send(WatchEvent::SocketUp) {
        return Ok(());
    }
    // A snapshot first, so we don't wait for the next change.
    if !poll_once(t).await {
        return Ok(());
    }
    while let Some(msg) = ws.next().await {
        let msg = msg.map_err(|e| e.to_string())?;
        let Message::Text(text) = msg else { continue };
        if let Some(msgs) = super::chat::messages_from_event(&text) {
            if !t.send(WatchEvent::Chat(msgs)) {
                return Ok(());
            }
            continue;
        }
        let Some(list) = presence::presences_from_event(&text) else {
            continue;
        };
        // Events carry only the presences that changed.
        let others: Vec<_> = list.iter().filter(|p| p.puuid != t.session.puuid).cloned().collect();
        if !others.is_empty() && !t.send(WatchEvent::Presences(others)) {
            return Ok(());
        }
        if !list.iter().any(|p| p.puuid == t.session.puuid) {
            continue;
        }
        let gp = presence::own_presence(&list, &t.session.puuid);
        if gp.is_none() {
            // Our entry changed but has no VALORANT presence (e.g. game closed).
            if !t.feed(None).await {
                return Ok(());
            }
            continue;
        }
        if !t.feed(gp).await {
            return Ok(());
        }
    }
    Ok(())
}
