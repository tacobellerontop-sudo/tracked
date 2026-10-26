//! Worker side of the optional backend: configuration, sign-in, the social
//! websocket (with reconnect + REST heartbeat), and DM/feedback intents.
//! Shared by the real supervisor and `--demo`.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use super::tasks::Ctx;
use crate::backend::models::{LobbyInfo, MinVersionState};
use crate::backend::social_ws::{self, ClientMsg, ServerMsg, SocketEvent};
use crate::backend::BackendClient;
use crate::error::{ApiError, UserFacingError};
use crate::ipc::{BackendStatus, Intent, Update};
use crate::riot::models::{LiveMatch, LivePhase};
use crate::riot::presence::{GamePresence, LoopState};

/// REST keep-alive cadence while signed in.
pub const REST_HEARTBEAT: Duration = Duration::from_secs(60);
const PAGE: u32 = 30;

/// Handle a backend-related intent. Returns `false` if it isn't one.
pub fn handle(ctx: &Arc<Ctx>, intent: &Intent) -> bool {
    let ctx = ctx.clone();
    match intent.clone() {
        Intent::BackendConfigure => {
            tokio::spawn(configure(ctx));
        }
        Intent::BackendSignIn { display_name } => {
            tokio::spawn(sign_in(ctx, display_name));
        }
        Intent::BackendSignOut => {
            tokio::spawn(sign_out(ctx));
        }
        Intent::SocialRefresh => {
            tokio::spawn(async move { report(&ctx, refresh(&ctx).await) });
        }
        Intent::DmOpen(conv) => {
            tokio::spawn(async move {
                let r = with_client(&ctx, |c| async move {
                    c.dm_page(&conv, None, PAGE).await.map(|p| (conv, p))
                })
                .await;
                match r {
                    Ok((conv, p)) => {
                        if let Some(last) = p.messages.last() {
                            let _ = with_client(&ctx, |c| {
                                let (conv, id) = (conv.clone(), last.id);
                                async move { c.dm_mark_read(&conv, id).await }
                            })
                            .await;
                        }
                        ctx.sink.send(Update::DmPage {
                            conversation_id: conv,
                            messages: p.messages,
                            has_more: p.has_more,
                        });
                    }
                    Err(e) => ctx.report(&e),
                }
            });
        }
        Intent::DmLoadOlder {
            conversation_id,
            before,
        } => {
            tokio::spawn(async move {
                let conv = conversation_id.clone();
                match with_client(&ctx, |c| async move { c.dm_page(&conv, Some(before), PAGE).await }).await {
                    Ok(p) => ctx.sink.send(Update::DmPage {
                        conversation_id,
                        messages: p.messages,
                        has_more: p.has_more,
                    }),
                    Err(e) => ctx.report(&e),
                }
            });
        }
        Intent::DmSend { conversation_id, text } => {
            tokio::spawn(async move {
                match with_client(&ctx, |c| async move { c.dm_send(&conversation_id, &text).await }).await {
                    Ok(m) => ctx.sink.send(Update::DmMessage(m)),
                    Err(e) => ctx.report(&e),
                }
            });
        }
        Intent::DmCreate { members, name } => {
            tokio::spawn(async move {
                let r = with_client(
                    &ctx,
                    |c| async move { c.dm_create_group(&members, name.as_deref()).await },
                )
                .await;
                match r {
                    Ok(conv) => ctx.sink.send(Update::DmConversation(conv)),
                    Err(e) => ctx.report(&e),
                }
            });
        }
        Intent::DmRename { conversation_id, name } => {
            tokio::spawn(async move {
                match with_client(&ctx, |c| async move { c.dm_rename(&conversation_id, &name).await }).await {
                    Ok(conv) => ctx.sink.send(Update::DmConversation(conv)),
                    Err(e) => ctx.report(&e),
                }
            });
        }
        Intent::DmMarkRead {
            conversation_id,
            message_id,
        } => {
            tokio::spawn(async move {
                let _ = with_client(
                    &ctx,
                    |c| async move { c.dm_mark_read(&conversation_id, message_id).await },
                )
                .await;
            });
        }
        Intent::DmTyping(conv) => {
            tokio::spawn(async move {
                let _ = with_client(&ctx, |c| async move { c.dm_typing(&conv).await }).await;
            });
        }
        Intent::SendFeedback(text) => {
            tokio::spawn(async move {
                match with_client(&ctx, |c| async move { c.feedback(&text).await }).await {
                    Ok(_) => ctx.sink.send(Update::Notice("Thanks — feedback sent.".into())),
                    Err(e) => ctx.report(&e),
                }
            });
        }
        _ => return false,
    }
    true
}

async fn with_client<T, F, Fut>(ctx: &Arc<Ctx>, f: F) -> Result<T, ApiError>
where
    F: FnOnce(Arc<BackendClient>) -> Fut,
    Fut: std::future::Future<Output = Result<T, ApiError>>,
{
    let client = ctx
        .backend
        .read()
        .unwrap()
        .clone()
        .ok_or(ApiError::Other("Social features are off".into()))?;
    if !client.signed_in() {
        return Err(ApiError::Other("Sign in to use social features".into()));
    }
    f(client).await
}

fn report(ctx: &Ctx, r: Result<(), ApiError>) {
    if let Err(e) = r {
        ctx.report(&e);
    }
}

/// Read the backend URL from settings, (re)build the client, check the
/// version gate and fetch remote config. Signs out of any previous backend.
pub async fn configure(ctx: Arc<Ctx>) {
    stop_socket(&ctx);
    let url = ctx
        .settings
        .lock()
        .unwrap()
        .backend_url
        .clone()
        .filter(|u| !u.trim().is_empty());
    let Some(url) = url else {
        *ctx.backend.write().unwrap() = None;
        ctx.sink.send(Update::Backend(BackendStatus::Disabled));
        ctx.sink.send(Update::MinVersion(MinVersionState::Ok));
        return;
    };
    let client = match BackendClient::new(&url) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            *ctx.backend.write().unwrap() = None;
            ctx.sink
                .send(Update::Backend(BackendStatus::Error(UserFacingError::from(&e).0)));
            return;
        }
    };
    *ctx.backend.write().unwrap() = Some(client.clone());
    ctx.sink.send(Update::Backend(BackendStatus::SignedOut));
    match client.min_version().await {
        Ok(min) => ctx.sink.send(Update::MinVersion(MinVersionState::evaluate(
            &min,
            env!("CARGO_PKG_VERSION"),
        ))),
        Err(e) => {
            tracing::warn!("min-version check failed: {e}");
            ctx.sink.send(Update::Backend(BackendStatus::Error(
                "Couldn't reach the backend".into(),
            )));
            return;
        }
    }
    if let Ok(cfg) = client.app_config().await {
        ctx.sink.send(Update::AppConfig(cfg));
    }
}

pub async fn sign_in(ctx: Arc<Ctx>, display_name: String) {
    let Some(client) = ctx.backend.read().unwrap().clone() else {
        ctx.sink.send(Update::Backend(BackendStatus::Error(
            "Set a backend URL in Settings first".into(),
        )));
        return;
    };
    ctx.sink.send(Update::Backend(BackendStatus::SigningIn));
    let riot_id = ctx.session().ok().map(|s| s.riot_id().to_string());
    match client.dev_login(display_name.trim(), riot_id.as_deref()).await {
        Ok(user) => {
            tracing::info!(user = %user.id, "signed in to backend");
            ctx.sink.send(Update::Backend(BackendStatus::SignedIn(user)));
            report(&ctx, refresh(&ctx).await);
            start_socket(&ctx, client);
        }
        Err(e) => ctx
            .sink
            .send(Update::Backend(BackendStatus::Error(UserFacingError::from(&e).0))),
    }
}

pub async fn sign_out(ctx: Arc<Ctx>) {
    stop_socket(&ctx);
    let client = ctx.backend.read().unwrap().clone();
    if let Some(c) = client {
        if c.signed_in() {
            let _ = c.lobby_clear().await;
        }
        c.sign_out();
        ctx.sink.send(Update::Backend(BackendStatus::SignedOut));
    }
}

/// Friends, their presence and lobbies, and the DM list.
pub async fn refresh(ctx: &Arc<Ctx>) -> Result<(), ApiError> {
    let friends = with_client(ctx, |c| async move { c.friends().await }).await?;
    let ids: Vec<String> = friends.iter().map(|f| f.id.clone()).collect();
    ctx.sink.send(Update::SocialFriends(friends));
    let ids2 = ids.clone();
    let presence = with_client(ctx, |c| async move { c.presence_lookup(&ids2).await }).await?;
    ctx.sink.send(Update::SocialPresence(presence));
    let lobbies = with_client(ctx, |c| async move { c.lobby_lookup(&ids).await }).await?;
    for (user_id, lobby) in lobbies {
        ctx.sink.send(Update::SocialLobby {
            user_id,
            lobby: Some(lobby),
        });
    }
    let convs = with_client(ctx, |c| async move { c.dm_list().await }).await?;
    ctx.sink.send(Update::DmConversations(convs));
    Ok(())
}

fn stop_socket(ctx: &Ctx) {
    *ctx.social_out.lock().unwrap() = None;
    if let Some(h) = ctx.social_task.lock().unwrap().take() {
        h.abort();
    }
}

/// Keep the social websocket up (reconnecting with backoff) and send the
/// REST heartbeat while signed in.
fn start_socket(ctx: &Arc<Ctx>, client: Arc<BackendClient>) {
    stop_socket(ctx);
    let ctx2 = ctx.clone();
    let handle = tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        while let Some(token) = client.token() {
            let (out_tx, out_rx) = mpsc::unbounded_channel();
            let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
            *ctx2.social_out.lock().unwrap() = Some(out_tx.clone());
            let topics = vec!["presence".into(), "lobby".into(), "dm".into()];
            let socket = tokio::spawn(social_ws::run(client.ws_url(), token, topics, out_rx, ev_tx));
            let mut rest_beat = tokio::time::interval(REST_HEARTBEAT);
            loop {
                tokio::select! {
                    ev = ev_rx.recv() => match ev {
                        Some(SocketEvent::Connected) => {
                            backoff = Duration::from_secs(1);
                            ctx2.sink.send(Update::SocialLive(true));
                            // Announce our current state and ask for friends' presence.
                            publish_current(&ctx2);
                            let _ = refresh(&ctx2).await;
                        }
                        Some(SocketEvent::Message(m)) => on_server_msg(&ctx2, m),
                        Some(SocketEvent::Closed(why)) => {
                            tracing::info!("social socket closed: {why}");
                            break;
                        }
                        None => break,
                    },
                    _ = rest_beat.tick() => {
                        let state = ctx2.game_state().map(|g| loop_name(g.loop_state)).unwrap_or("OFFLINE");
                        if let Err(e) = client.heartbeat(state).await {
                            tracing::debug!("heartbeat failed: {e}");
                        }
                    }
                }
            }
            socket.abort();
            ctx2.sink.send(Update::SocialLive(false));
            if !client.signed_in() {
                break;
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }
    });
    *ctx.social_task.lock().unwrap() = Some(handle);
}

fn on_server_msg(ctx: &Ctx, m: ServerMsg) {
    match m {
        ServerMsg::Presence { entries } => ctx.sink.send(Update::SocialPresence(entries)),
        ServerMsg::Lobby { user_id, lobby } => ctx.sink.send(Update::SocialLobby { user_id, lobby }),
        ServerMsg::Dm { message, .. } => ctx.sink.send(Update::DmMessage(message)),
        ServerMsg::Typing {
            conversation_id,
            user_id,
        } => ctx.sink.send(Update::DmTyping {
            conversation_id,
            user_id,
        }),
        ServerMsg::Conversation { conversation } => ctx.sink.send(Update::DmConversation(conversation)),
        ServerMsg::Error { message } => tracing::warn!("social socket error: {message}"),
        _ => {}
    }
}

pub fn loop_name(s: LoopState) -> &'static str {
    match s {
        LoopState::Menus => "MENUS",
        LoopState::Pregame => "PREGAME",
        LoopState::InGame => "INGAME",
        LoopState::Unknown => "UNKNOWN",
    }
}

fn send(ctx: &Ctx, m: ClientMsg) {
    if let Some(tx) = ctx.social_out.lock().unwrap().as_ref() {
        let _ = tx.send(m);
    }
}

/// Publish our presence (if the user allows it) — called on game state changes.
pub fn publish_presence(ctx: &Ctx, g: &GamePresence) {
    if !ctx.settings.lock().unwrap().share_presence {
        return;
    }
    send(
        ctx,
        ClientMsg::PresenceUsage {
            state: loop_name(g.loop_state).into(),
            queue: g.queue_id.clone(),
            party_size: g.party_size,
        },
    );
}

/// Publish (or clear) our lobby summary — called when the party changes.
pub fn publish_lobby(ctx: &Ctx, live: Option<&LiveMatch>) {
    if !ctx.settings.lock().unwrap().share_lobby {
        return;
    }
    let lobby = live.filter(|m| m.phase == LivePhase::Party).map(|m| LobbyInfo {
        queue: m.queue_id.clone(),
        size: m.players.len() as u32,
        max: 5,
        open: m.players.len() < 5,
    });
    send(ctx, ClientMsg::LobbyPush { lobby });
}

fn publish_current(ctx: &Ctx) {
    if let Some(g) = ctx.game_state() {
        publish_presence(ctx, &g);
    }
    let live = ctx.live.lock().unwrap().clone();
    publish_lobby(ctx, live.as_ref());
}
