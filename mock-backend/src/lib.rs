//! An in-memory mock of the optional Tracked backend, implementing the
//! contract the app's `backend/` client speaks:
//!
//! * auth: `POST /api/auth/dev-login` issues an HS256 JWT; every other route
//!   wants `Authorization: Bearer <jwt>`
//! * companion: app-config, min-version, heartbeat, feedback, lobby
//!   (publish/touch/lookup/clear), presence lookup, crosshairs, rewards,
//!   profile/account
//! * social REST: friends + DMs (list, page, send, create group, rename, mark
//!   read, typing, report, request respond)
//! * social websocket `/social/ws`: subscribe, heartbeat, presence_lookup,
//!   presence_usage, lobby_push, lobby_touch
//! * stats service (`/mm/...`): canned responses
//!
//! Three bot friends exist for every user; bots answer DMs so the UI can be
//! exercised by one person offline. Nothing here talks to Riot.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;

const SECRET: &[u8] = b"mock-backend-dev-secret";

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    name: String,
    exp: u64,
}

#[derive(Clone, Serialize)]
struct User {
    id: String,
    display_name: String,
    riot_id: Option<String>,
    bot: bool,
}

#[derive(Clone, Serialize)]
struct Msg {
    id: u64,
    conversation_id: String,
    sender_id: String,
    text: String,
    sent_at: u64,
}

#[derive(Clone)]
struct Conversation {
    id: String,
    name: Option<String>,
    members: Vec<String>,
    messages: Vec<Msg>,
    read_upto: HashMap<String, u64>,
}

#[derive(Default)]
struct Inner {
    users: HashMap<String, User>,
    convs: HashMap<String, Conversation>,
    presence: HashMap<String, Value>,
    lobbies: HashMap<String, Value>,
    sockets: HashMap<String, Vec<mpsc::UnboundedSender<String>>>,
    feedback: Vec<String>,
    next_id: u64,
}

#[derive(Clone, Default)]
pub struct AppState(Arc<Mutex<Inner>>);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

const BOTS: &[(&str, &str, &str)] = &[
    ("bot-nimbus", "Nimbus", "INGAME"),
    ("bot-kettle", "kettle", "MENUS"),
    ("bot-orchid", "Orchid", "OFFLINE"),
];

impl Inner {
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn ensure_bots(&mut self) {
        for (id, name, state) in BOTS {
            self.users.entry(id.to_string()).or_insert_with(|| User {
                id: id.to_string(),
                display_name: name.to_string(),
                riot_id: Some(format!("{name}#BOT")),
                bot: true,
            });
            if *state != "OFFLINE" {
                self.presence.entry(id.to_string()).or_insert_with(|| {
                    json!({ "user_id": id, "online": true, "state": state, "queue": "competitive",
                            "party_size": if *state == "MENUS" { 2 } else { 1 }, "using_app": true })
                });
            }
        }
        self.lobbies.entry("bot-kettle".into()).or_insert_with(
            || json!({ "party_id": "mock-party-kettle", "queue": "competitive", "size": 2, "max": 5, "open": true }),
        );
    }

    fn push(&mut self, user: &str, event: Value) {
        let text = event.to_string();
        if let Some(socks) = self.sockets.get_mut(user) {
            socks.retain(|s| s.send(text.clone()).is_ok());
        }
    }

    fn push_all(&mut self, event: Value) {
        let users: Vec<String> = self.sockets.keys().cloned().collect();
        for u in users {
            self.push(&u, event.clone());
        }
    }

    fn conv_json(&self, c: &Conversation, viewer: &str) -> Value {
        let read = c.read_upto.get(viewer).copied().unwrap_or(0);
        let unread = c
            .messages
            .iter()
            .filter(|m| m.id > read && m.sender_id != viewer)
            .count();
        let names: Vec<String> = c
            .members
            .iter()
            .filter(|m| *m != viewer)
            .filter_map(|m| self.users.get(m).map(|u| u.display_name.clone()))
            .collect();
        json!({ "id": c.id, "name": c.name, "members": c.members, "title": c.name.clone().unwrap_or_else(|| names.join(", ")),
                "last_message": c.messages.last(), "unread": unread, "is_group": c.members.len() > 2 })
    }

    fn post_message(&mut self, conv_id: &str, sender: &str, text: &str) -> Option<Msg> {
        let id = self.id();
        let conv = self.convs.get_mut(conv_id)?;
        let msg = Msg {
            id,
            conversation_id: conv_id.into(),
            sender_id: sender.into(),
            text: text.into(),
            sent_at: now(),
        };
        conv.messages.push(msg.clone());
        conv.read_upto.insert(sender.into(), id);
        let members = conv.members.clone();
        for m in members {
            self.push(&m, json!({ "type": "dm", "conversation_id": conv_id, "message": msg }));
        }
        Some(msg)
    }
}

pub fn app() -> Router {
    app_with_state(AppState::default())
}

pub fn app_with_state(state: AppState) -> Router {
    state.0.lock().unwrap().ensure_bots();
    Router::new()
        .route("/api/auth/dev-login", post(dev_login))
        .route("/api/account", get(account))
        .route(
            "/api/account/analytics-preference",
            get(analytics_get).post(analytics_set),
        )
        .route("/api/profile/check", get(profile_check))
        .route("/api/rewards", get(rewards))
        .route("/api/crosshairs", get(crosshairs))
        .route("/api/companion/app-config", get(app_config))
        .route("/api/companion/min-version", get(min_version))
        .route("/api/companion/heartbeat", post(heartbeat))
        .route("/api/companion/feedback", post(feedback))
        .route("/api/companion/lobby", post(lobby_push))
        .route("/api/companion/lobby/touch", post(lobby_touch))
        .route("/api/companion/lobby/lookup", post(lobby_lookup))
        .route("/api/companion/lobby/clear", post(lobby_clear))
        .route("/api/companion/presence/lookup", post(presence_lookup))
        .route("/social/v1/friends", get(friends))
        .route("/social/v1/dm", get(dm_list).post(dm_create_group))
        .route("/social/v1/dm/:id/messages", get(dm_page).post(dm_send))
        .route("/social/v1/dm/:id/rename", post(dm_rename))
        .route("/social/v1/dm/:id/read", post(dm_mark_read))
        .route("/social/v1/dm/:id/typing", post(dm_typing))
        .route("/social/v1/dm/:id/report", post(dm_report))
        .route("/social/v1/dm/requests/:id", post(dm_request_respond))
        .route("/social/ws", get(social_ws))
        .route("/mm/v1/matches/:id", get(mm_match))
        .route("/mm/v1/history/:puuid", get(mm_history))
        .route("/mm/v1/players/:puuid", get(mm_player))
        .route("/mm/v2/players/:puuid", get(mm_player))
        .route("/mm/v1/parties/:id", get(mm_party))
        .route("/mm/v1/seasons", get(mm_seasons))
        .route("/mm/v2/namesets-for-puuids", post(mm_namesets))
        .with_state(state)
}

pub async fn serve(listener: tokio::net::TcpListener) {
    axum::serve(listener, app()).await.expect("server error");
}

// ------------------------------------------------------------------ auth --

struct ApiError(StatusCode, &'static str);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn issue(user: &User) -> String {
    let claims = Claims {
        sub: user.id.clone(),
        name: user.display_name.clone(),
        exp: now() + 12 * 3600,
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(SECRET),
    )
    .unwrap()
}

fn verify(token: &str) -> ApiResult<String> {
    jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(SECRET),
        &jsonwebtoken::Validation::default(),
    )
    .map(|d| d.claims.sub)
    .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "invalid token"))
}

fn auth(headers: &HeaderMap) -> ApiResult<String> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "missing bearer token"))?;
    verify(token)
}

#[derive(Deserialize)]
struct LoginReq {
    display_name: String,
    riot_id: Option<String>,
}

async fn dev_login(State(st): State<AppState>, Json(req): Json<LoginReq>) -> ApiResult<Json<Value>> {
    let name = req.display_name.trim();
    if name.is_empty() || name.len() > 32 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "display_name must be 1-32 chars"));
    }
    let mut s = st.0.lock().unwrap();
    let id = format!("user-{}", name.to_lowercase().replace(' ', "-"));
    let user = User {
        id: id.clone(),
        display_name: name.into(),
        riot_id: req.riot_id,
        bot: false,
    };
    s.users.insert(id.clone(), user.clone());
    // Seed a conversation with a bot so the DM list isn't empty.
    let conv_id = format!("dm-{id}-bot-nimbus");
    if !s.convs.contains_key(&conv_id) {
        s.convs.insert(
            conv_id.clone(),
            Conversation {
                id: conv_id.clone(),
                name: None,
                members: vec![id.clone(), "bot-nimbus".into()],
                messages: Vec::new(),
                read_upto: HashMap::new(),
            },
        );
        s.post_message(&conv_id, "bot-nimbus", "gg last night! queue later?");
    }
    Ok(Json(json!({ "token": issue(&user), "user": user })))
}

async fn account(State(st): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    let s = st.0.lock().unwrap();
    Ok(Json(json!({ "user": s.users.get(&me) })))
}

async fn analytics_get(h: HeaderMap) -> ApiResult<Json<Value>> {
    auth(&h)?;
    Ok(Json(json!({ "analytics": false })))
}

async fn analytics_set(h: HeaderMap, Json(v): Json<Value>) -> ApiResult<Json<Value>> {
    auth(&h)?;
    Ok(Json(
        json!({ "analytics": v.get("analytics").cloned().unwrap_or(json!(false)) }),
    ))
}

async fn profile_check(h: HeaderMap) -> ApiResult<Json<Value>> {
    auth(&h)?;
    Ok(Json(json!({ "complete": true })))
}

async fn rewards(h: HeaderMap) -> ApiResult<Json<Value>> {
    auth(&h)?;
    Ok(Json(json!({ "rewards": [] })))
}

async fn crosshairs() -> Json<Value> {
    Json(json!({ "crosshairs": [
        { "name": "Tiny cyan", "code": "0;P;c;5;h;0;0l;4;0o;2;0a;1;0f;0;1b;0", "author": "community" },
        { "name": "Dot", "code": "0;P;c;1;h;1;d;1;z;3;0b;0;1b;0", "author": "community" }
    ]}))
}

// ------------------------------------------------------------- companion --

async fn app_config() -> Json<Value> {
    Json(
        json!({ "features": { "social": true, "dms": true, "lobbies": true, "screenshare": false },
                 "motd": "You're connected to the local mock backend." }),
    )
}

async fn min_version() -> Json<Value> {
    let min = std::env::var("MOCK_MIN_VERSION").unwrap_or_else(|_| "0.1.0".into());
    Json(json!({ "min_version": min, "download_url": "https://github.com/tacobellerontop-sudo/tracked/releases" }))
}

async fn heartbeat(State(st): State<AppState>, h: HeaderMap, Json(v): Json<Value>) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    let mut s = st.0.lock().unwrap();
    let entry = s
        .presence
        .entry(me.clone())
        .or_insert_with(|| json!({ "user_id": me, "using_app": true }));
    entry["online"] = json!(true);
    entry["last_heartbeat"] = json!(now());
    if let Some(version) = v.get("version") {
        entry["version"] = version.clone();
    }
    Ok(Json(json!({ "ok": true, "server_time": now() })))
}

async fn feedback(State(st): State<AppState>, h: HeaderMap, Json(v): Json<Value>) -> ApiResult<Json<Value>> {
    auth(&h)?;
    let text = v.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
    if text.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "empty feedback"));
    }
    st.0.lock().unwrap().feedback.push(text);
    Ok(Json(json!({ "ok": true })))
}

fn set_lobby(s: &mut Inner, me: &str, lobby: Option<Value>) {
    match &lobby {
        Some(l) => s.lobbies.insert(me.into(), l.clone()),
        None => s.lobbies.remove(me),
    };
    s.push_all(json!({ "type": "lobby", "user_id": me, "lobby": lobby }));
}

async fn lobby_push(State(st): State<AppState>, h: HeaderMap, Json(v): Json<Value>) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    set_lobby(&mut st.0.lock().unwrap(), &me, Some(v));
    Ok(Json(json!({ "ok": true })))
}

async fn lobby_touch(h: HeaderMap) -> ApiResult<Json<Value>> {
    auth(&h)?;
    Ok(Json(json!({ "ok": true })))
}

async fn lobby_clear(State(st): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    set_lobby(&mut st.0.lock().unwrap(), &me, None);
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct UserIds {
    user_ids: Vec<String>,
}

async fn lobby_lookup(State(st): State<AppState>, h: HeaderMap, Json(req): Json<UserIds>) -> ApiResult<Json<Value>> {
    auth(&h)?;
    let s = st.0.lock().unwrap();
    let lobbies: Vec<Value> = req
        .user_ids
        .iter()
        .filter_map(|u| s.lobbies.get(u).map(|l| json!({ "user_id": u, "lobby": l })))
        .collect();
    Ok(Json(json!({ "lobbies": lobbies })))
}

fn presence_entries(s: &Inner, ids: &[String]) -> Vec<Value> {
    ids.iter()
        .map(|u| {
            s.presence
                .get(u)
                .cloned()
                .unwrap_or_else(|| json!({ "user_id": u, "online": false }))
        })
        .collect()
}

async fn presence_lookup(State(st): State<AppState>, h: HeaderMap, Json(req): Json<UserIds>) -> ApiResult<Json<Value>> {
    auth(&h)?;
    let s = st.0.lock().unwrap();
    Ok(Json(json!({ "entries": presence_entries(&s, &req.user_ids) })))
}

// ---------------------------------------------------------------- social --

async fn friends(State(st): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    let s = st.0.lock().unwrap();
    // Everyone else is your friend in the mock.
    let list: Vec<&User> = s.users.values().filter(|u| u.id != me).collect();
    Ok(Json(json!({ "friends": list })))
}

fn member_of(s: &Inner, conv: &str, me: &str) -> ApiResult<()> {
    match s.convs.get(conv) {
        Some(c) if c.members.iter().any(|m| m == me) => Ok(()),
        Some(_) => Err(ApiError(StatusCode::FORBIDDEN, "not a member")),
        None => Err(ApiError(StatusCode::NOT_FOUND, "no such conversation")),
    }
}

async fn dm_list(State(st): State<AppState>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    let s = st.0.lock().unwrap();
    let mut convs: Vec<&Conversation> = s.convs.values().filter(|c| c.members.contains(&me)).collect();
    convs.sort_by_key(|c| std::cmp::Reverse(c.messages.last().map(|m| m.id).unwrap_or(0)));
    let list: Vec<Value> = convs.iter().map(|c| s.conv_json(c, &me)).collect();
    Ok(Json(json!({ "conversations": list })))
}

#[derive(Deserialize)]
struct CreateReq {
    members: Vec<String>,
    name: Option<String>,
}

async fn dm_create_group(
    State(st): State<AppState>,
    h: HeaderMap,
    Json(req): Json<CreateReq>,
) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    let mut s = st.0.lock().unwrap();
    let mut members = req.members.clone();
    members.retain(|m| m != &me && s.users.contains_key(m));
    if members.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "no valid members"));
    }
    members.push(me.clone());
    members.sort();
    members.dedup();
    // Reuse an existing 1:1 conversation.
    if members.len() == 2 && req.name.is_none() {
        if let Some(c) = s
            .convs
            .values()
            .find(|c| c.members.len() == 2 && members.iter().all(|m| c.members.contains(m)))
        {
            return Ok(Json(s.conv_json(c, &me)));
        }
    }
    let id = format!("dm-{}", s.id());
    let conv = Conversation {
        id: id.clone(),
        name: req.name,
        members: members.clone(),
        messages: vec![],
        read_upto: HashMap::new(),
    };
    let view = s.conv_json(&conv, &me);
    s.convs.insert(id.clone(), conv);
    for m in &members {
        let v = s.conv_json(&s.convs[&id].clone(), m);
        s.push(m, json!({ "type": "conversation", "conversation": v }));
    }
    Ok(Json(view))
}

#[derive(Deserialize)]
struct PageQuery {
    before: Option<u64>,
    limit: Option<usize>,
}

async fn dm_page(
    State(st): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<PageQuery>,
) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    let s = st.0.lock().unwrap();
    member_of(&s, &id, &me)?;
    let limit = q.limit.unwrap_or(30).min(100);
    let all: Vec<&Msg> = s.convs[&id]
        .messages
        .iter()
        .filter(|m| q.before.is_none_or(|b| m.id < b))
        .collect();
    let start = all.len().saturating_sub(limit);
    Ok(Json(json!({ "messages": &all[start..], "has_more": start > 0 })))
}

#[derive(Deserialize)]
struct SendReq {
    text: String,
}

async fn dm_send(
    State(st): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<SendReq>,
) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    let text = req.text.trim().to_string();
    if text.is_empty() || text.len() > 2000 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "message must be 1-2000 chars"));
    }
    let (msg, bots) = {
        let mut s = st.0.lock().unwrap();
        member_of(&s, &id, &me)?;
        let msg = s.post_message(&id, &me, &text).unwrap();
        let bots: Vec<String> = s.convs[&id]
            .members
            .iter()
            .filter(|m| m.starts_with("bot-"))
            .cloned()
            .collect();
        (msg, bots)
    };
    // Bots "type" for a moment, then reply.
    for bot in bots {
        let (st, id, text) = (st.clone(), id.clone(), text.clone());
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            {
                let mut s = st.0.lock().unwrap();
                let members = s.convs.get(&id).map(|c| c.members.clone()).unwrap_or_default();
                for m in members {
                    s.push(&m, json!({ "type": "typing", "conversation_id": id, "user_id": bot }));
                }
            }
            tokio::time::sleep(Duration::from_millis(900)).await;
            let mut s = st.0.lock().unwrap();
            let name = s.users.get(&bot).map(|u| u.display_name.clone()).unwrap_or_default();
            s.post_message(&id, &bot, &format!("{name} (bot) heard: \"{text}\""));
        });
    }
    Ok(Json(json!(msg)))
}

#[derive(Deserialize)]
struct RenameReq {
    name: String,
}

async fn dm_rename(
    State(st): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<RenameReq>,
) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    let mut s = st.0.lock().unwrap();
    member_of(&s, &id, &me)?;
    let name = req.name.trim().to_string();
    s.convs.get_mut(&id).unwrap().name = (!name.is_empty()).then_some(name);
    let conv = s.convs[&id].clone();
    for m in &conv.members {
        let v = s.conv_json(&conv, m);
        s.push(m, json!({ "type": "conversation", "conversation": v }));
    }
    Ok(Json(s.conv_json(&conv, &me)))
}

#[derive(Deserialize)]
struct ReadReq {
    message_id: u64,
}

async fn dm_mark_read(
    State(st): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<ReadReq>,
) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    let mut s = st.0.lock().unwrap();
    member_of(&s, &id, &me)?;
    s.convs.get_mut(&id).unwrap().read_upto.insert(me, req.message_id);
    Ok(Json(json!({ "ok": true })))
}

async fn dm_typing(State(st): State<AppState>, h: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    let mut s = st.0.lock().unwrap();
    member_of(&s, &id, &me)?;
    let members = s.convs[&id].members.clone();
    for m in members.iter().filter(|m| **m != me) {
        s.push(m, json!({ "type": "typing", "conversation_id": id, "user_id": me }));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn dm_report(State(st): State<AppState>, h: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let me = auth(&h)?;
    member_of(&st.0.lock().unwrap(), &id, &me)?;
    Ok(Json(json!({ "ok": true, "status": "received" })))
}

async fn dm_request_respond(h: HeaderMap, Path(_id): Path<String>, Json(v): Json<Value>) -> ApiResult<Json<Value>> {
    auth(&h)?;
    Ok(Json(
        json!({ "ok": true, "accepted": v.get("accept").cloned().unwrap_or(json!(false)) }),
    ))
}

// ------------------------------------------------------------- websocket --

async fn social_ws(State(st): State<AppState>, h: HeaderMap, ws: WebSocketUpgrade) -> Response {
    match auth(&h) {
        Ok(me) => ws.on_upgrade(move |socket| ws_session(st, me, socket)),
        Err(e) => e.into_response(),
    }
}

async fn ws_session(st: AppState, me: String, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    st.0.lock()
        .unwrap()
        .sockets
        .entry(me.clone())
        .or_default()
        .push(tx.clone());
    let _ = tx.send(json!({ "type": "hello", "user_id": me }).to_string());

    let writer = tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            if sink.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = stream.next().await {
        let Message::Text(text) = msg else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            let _ = tx.send(json!({ "type": "error", "message": "invalid json" }).to_string());
            continue;
        };
        let mut s = st.0.lock().unwrap();
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "subscribe" => {
                let _ = tx.send(json!({ "type": "subscribed", "topics": v.get("topics") }).to_string());
            }
            "heartbeat" => {
                let _ = tx.send(json!({ "type": "heartbeat_ack", "server_time": now() }).to_string());
            }
            "presence_lookup" => {
                let ids: Vec<String> = serde_json::from_value(v["user_ids"].clone()).unwrap_or_default();
                let mut entries = presence_entries(&s, &ids);
                for e in &mut entries {
                    if let Some(l) = e["user_id"].as_str().and_then(|u| s.lobbies.get(u)) {
                        e["lobby"] = l.clone();
                    }
                }
                let _ = tx.send(json!({ "type": "presence", "entries": entries }).to_string());
            }
            "presence_usage" => {
                let mut entry = json!({ "user_id": me, "online": true, "using_app": true });
                for k in ["state", "queue", "party_size"] {
                    if let Some(x) = v.get(k) {
                        entry[k] = x.clone();
                    }
                }
                s.presence.insert(me.clone(), entry.clone());
                s.push_all(json!({ "type": "presence", "entries": [entry] }));
            }
            "lobby_push" => {
                let lobby = v.get("lobby").cloned().filter(|l| !l.is_null());
                set_lobby(&mut s, &me, lobby);
            }
            "lobby_touch" => {
                let _ = tx.send(json!({ "type": "lobby_touched" }).to_string());
            }
            other => {
                let _ = tx.send(json!({ "type": "error", "message": format!("unknown type {other:?}") }).to_string());
            }
        }
    }

    writer.abort();
    let mut s = st.0.lock().unwrap();
    if let Some(socks) = s.sockets.get_mut(&me) {
        socks.retain(|t| !t.same_channel(&tx));
    }
    if s.sockets.get(&me).is_some_and(|v| v.is_empty()) {
        s.sockets.remove(&me);
        s.presence.insert(me.clone(), json!({ "user_id": me, "online": false }));
        s.push_all(json!({ "type": "presence", "entries": [{ "user_id": me, "online": false }] }));
    }
}

// --------------------------------------------------------- stats (canned) --

async fn mm_match(h: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    auth(&h)?;
    Ok(Json(
        json!({ "matchInfo": { "matchId": id, "mapId": "/Game/Maps/Ascent/Ascent", "queueId": "competitive",
        "gameLengthMs": 1_800_000, "startedAtUnix": now() - 7200, "isRanked": true, "isCompleted": true },
        "players": [], "teams": [], "roundResults": [] }),
    ))
}

async fn mm_history(h: HeaderMap, Path(puuid): Path<String>) -> ApiResult<Json<Value>> {
    auth(&h)?;
    Ok(Json(json!({ "puuid": puuid, "matches": [] })))
}

async fn mm_player(h: HeaderMap, Path(puuid): Path<String>) -> ApiResult<Json<Value>> {
    auth(&h)?;
    Ok(Json(
        json!({ "puuid": puuid, "competitiveTier": 0, "rankedRating": 0, "matchesTracked": 0 }),
    ))
}

async fn mm_party(h: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    auth(&h)?;
    Ok(Json(json!({ "partyId": id, "members": [] })))
}

async fn mm_seasons() -> Json<Value> {
    Json(json!({ "seasons": [] }))
}

async fn mm_namesets(h: HeaderMap, Json(v): Json<Value>) -> ApiResult<Json<Value>> {
    auth(&h)?;
    let puuids = v.get("puuids").and_then(Value::as_array).cloned().unwrap_or_default();
    if puuids.len() > 100 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "too many puuids"));
    }
    Ok(Json(json!({ "names": [] })))
}
