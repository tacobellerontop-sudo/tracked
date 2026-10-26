//! A fake Riot Client local API for integration tests: self-signed TLS on
//! 127.0.0.1, HTTP basic auth, a handful of JSON endpoints and the presence
//! websocket. Nothing here talks to the internet.

#![allow(dead_code)]

use std::sync::Arc;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::tungstenite::Message;

pub const PASSWORD: &str = "PASSWORD-SENTINEL-7f3a";
pub const ACCESS_TOKEN: &str = "ACCESS-SENTINEL-91bc";
pub const ENT_TOKEN: &str = "ENTITLEMENT-SENTINEL-55de";
pub const PUUID: &str = "0b0e4a2c-1111-4b6c-9c6e-123456789abc";
pub const CLIENT_VERSION: &str = "release-test";
pub const MATCH_ID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
pub const PREGAME_ID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
pub const CORE_ID: &str = "cccccccc-0000-4000-8000-000000000003";
pub const PARTY_ID: &str = "dddddddd-0000-4000-8000-000000000004";
/// Index of the in-game player with streamer mode on.
pub const INCOGNITO: usize = 7;

/// Player `i` of the fake lobby (0 is us).
pub fn player(i: usize) -> String {
    if i == 0 {
        PUUID.to_string()
    } else {
        format!("0b0e4a2c-0000-4b6c-9c6e-{i:012}")
    }
}

fn index_of(puuid: &str) -> usize {
    (0..10).find(|i| player(*i) == puuid).unwrap_or(0)
}

/// Bump to rotate the access token: the old one then gets 401 from PD/GLZ
/// and entitlements hands out the new one (tests token refresh).
pub static TOKEN_GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Set to false to make the chat session report "not signed in".
pub static SIGNED_IN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// The access token the fake server currently accepts.
pub fn access_token() -> String {
    format!("{ACCESS_TOKEN}-{}", TOKEN_GEN.load(std::sync::atomic::Ordering::SeqCst))
}

/// Every PUUID the app asked the name service about.
pub static NAME_REQUESTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn private_blob(state: &str) -> String {
    let json = serde_json::json!({
        "isValid": true,
        "matchPresenceData": { "sessionLoopState": state, "queueId": "competitive", "matchMap": "/Game/Maps/Ascent/Ascent" },
        "partyPresenceData": { "partyId": "party-1", "partySize": 2 }
    });
    base64::engine::general_purpose::STANDARD.encode(json.to_string())
}

fn presences(state: &str) -> serde_json::Value {
    serde_json::json!({ "presences": [
        { "puuid": "someone-else", "product": "valorant", "private": private_blob("MENUS") },
        { "puuid": PUUID, "product": "league_of_legends", "private": "" },
        { "puuid": PUUID, "product": "valorant", "game_name": "Tester", "game_tag": "0001", "private": private_blob(state) }
    ]})
}

fn identity(i: usize) -> serde_json::Value {
    serde_json::json!({ "AccountLevel": 100 + i, "Incognito": i == INCOGNITO, "HideAccountLevel": i == INCOGNITO })
}

/// PD / GLZ routes (bearer-authenticated).
fn riot_route(method: &str, path: &str, body: &str) -> Option<serde_json::Value> {
    let seg: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    Some(match (method, seg.as_slice()) {
        ("GET", ["mmr", "v1", "players", p]) => {
            let i = index_of(p);
            serde_json::json!({ "Subject": p,
                "LatestCompetitiveUpdate": { "SeasonID": "act-1", "TierAfterUpdate": 10 + i, "RankedRatingAfterUpdate": 42 },
                "QueueSkills": { "competitive": { "SeasonalInfoBySeasonID": { "act-1": {
                    "SeasonID": "act-1", "NumberOfWins": 10, "NumberOfGames": 20, "CompetitiveTier": 10 + i,
                    "RankedRating": 42, "WinsByTier": { "21": 2 } } } } } })
        }
        ("GET", ["mmr", "v1", "players", p, "competitiveupdates"]) => serde_json::json!({ "Subject": p, "Matches": [
            { "MatchID": MATCH_ID, "SeasonID": "act-1", "MatchStartTime": 1_700_000_000_000i64, "TierAfterUpdate": 12,
              "RankedRatingAfterUpdate": 42, "RankedRatingEarned": 18 },
            { "MatchID": MATCH_ID, "SeasonID": "act-1", "MatchStartTime": 1_699_990_000_000i64, "TierAfterUpdate": 12,
              "RankedRatingAfterUpdate": 24, "RankedRatingEarned": -15 }
        ]}),
        ("GET", ["match-history", "v1", "history", p]) => serde_json::json!({ "Subject": p, "Total": 1,
            "History": [{ "MatchID": MATCH_ID, "GameStartTime": 1_700_000_000_000i64, "QueueID": "competitive" }] }),
        ("GET", ["match-details", "v1", "matches", MATCH_ID]) => serde_json::json!({
            "matchInfo": { "matchId": MATCH_ID, "mapId": "/Game/Maps/Ascent/Ascent", "queueID": "competitive",
                           "gameLengthMillis": 1_800_000, "gameStartMillis": 1_700_000_000_000i64, "isCompleted": true },
            "players": (0..10).map(|i| serde_json::json!({ "subject": player(i), "gameName": format!("Player{i}"), "tagLine": "TAG",
                "teamId": if i < 5 { "Blue" } else { "Red" }, "characterId": "agent",
                "stats": { "score": 3000, "roundsPlayed": 20, "kills": 15, "deaths": 12, "assists": 4 } })).collect::<Vec<_>>(),
            "teams": [{ "teamId": "Blue", "won": true, "roundsWon": 13, "roundsPlayed": 20 },
                      { "teamId": "Red", "won": false, "roundsWon": 7, "roundsPlayed": 20 }],
            "roundResults": []
        }),
        ("PUT", ["name-service", "v2", "players"]) => {
            let asked: Vec<String> = serde_json::from_str(body).unwrap_or_default();
            NAME_REQUESTS.lock().unwrap().extend(asked.iter().cloned());
            serde_json::Value::Array(
                asked
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                "Subject": p, "GameName": format!("Player{}", index_of(p)), "TagLine": "TAG", "DisplayName": "" })
                    })
                    .collect(),
            )
        }
        ("GET", ["pregame", "v1", "players", _]) => serde_json::json!({ "MatchID": PREGAME_ID }),
        ("GET", ["pregame", "v1", "matches", PREGAME_ID]) => serde_json::json!({
            "ID": PREGAME_ID, "MapID": "/Game/Maps/Ascent/Ascent", "QueueID": "competitive",
            "AllyTeam": { "TeamID": "Blue", "Players": (0..5).map(|i| serde_json::json!({
                "Subject": player(i), "CharacterID": if i < 3 { "agent" } else { "" },
                "CharacterSelectionState": if i < 2 { "locked" } else { "selected" }, "PlayerIdentity": identity(i) })).collect::<Vec<_>>() }
        }),
        ("GET", ["core-game", "v1", "players", _]) => serde_json::json!({ "MatchID": CORE_ID }),
        ("GET", ["core-game", "v1", "matches", CORE_ID]) => serde_json::json!({
            "MatchID": CORE_ID, "MapID": "/Game/Maps/Ascent/Ascent", "GamePodID": "aresriot.aws-euc1-prod.eu-gp-frankfurt-1",
            "MatchmakingData": { "QueueID": "competitive" },
            "Players": (0..10).map(|i| serde_json::json!({ "Subject": player(i), "TeamID": if i < 5 { "Blue" } else { "Red" },
                "CharacterID": "agent", "PlayerIdentity": identity(i) })).collect::<Vec<_>>()
        }),
        ("GET", ["parties", "v1", "players", _]) => serde_json::json!({ "CurrentPartyID": PARTY_ID }),
        ("GET", ["parties", "v1", "parties", PARTY_ID]) => serde_json::json!({ "ID": PARTY_ID,
            "Members": [{ "Subject": PUUID, "PlayerIdentity": identity(0) }, { "Subject": player(1), "PlayerIdentity": identity(1) }] }),
        _ => return None,
    })
}

fn is_riot_path(path: &str) -> bool {
    [
        "/mmr/",
        "/match-history/",
        "/match-details/",
        "/name-service/",
        "/pregame/",
        "/core-game/",
        "/parties/",
    ]
    .iter()
    .any(|p| path.starts_with(p))
}

fn route(path: &str) -> Option<serde_json::Value> {
    Some(match path {
        "/entitlements/v1/token" => serde_json::json!({
            "accessToken": access_token(), "token": ENT_TOKEN, "subject": PUUID, "entitlements": [], "issuer": "test"
        }),
        "/chat/v1/session" => serde_json::json!({
            "puuid": PUUID, "game_name": "Tester", "game_tag": "0001", "region": "eu1",
            "loaded": SIGNED_IN.load(std::sync::atomic::Ordering::SeqCst)
        }),
        "/product-session/v1/external-sessions" => serde_json::json!({
            "host_app": { "productId": "riot_client", "launchConfiguration": { "arguments": [] } },
            "1234": { "productId": "valorant", "launchConfiguration": {
                "arguments": ["-ares-deployment=eu", "-config-endpoint=https://shared.eu.a.pvp.net"] } }
        }),
        "/riotclient/region-locale" => serde_json::json!({ "locale": "en_GB", "region": "EUW", "webRegion": "eu" }),
        "/chat/v4/presences" => presences("MENUS"),
        "/chat/v4/friends" => serde_json::json!({ "friends": [
            { "puuid": "friend-puuid", "game_name": "Buddy", "game_tag": "EUW" }
        ]}),
        _ => return None,
    })
}

/// Start the fake server; returns the bound port.
pub async fn start() -> u16 {
    let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into());
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { break };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(tls) = acceptor.accept(tcp).await {
                    let _ = serve(tls).await;
                }
            });
        }
    });
    port
}

async fn serve<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(mut s: S) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = s.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    let method = head.split_whitespace().next().unwrap_or("GET").to_string();
    let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
    let header = |name: &str| {
        head.lines().find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim().to_string())
        })
    };
    let mut body = buf[split..].to_vec();
    let len: usize = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    while body.len() < len {
        let n = s.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    let clean_path = path.split('?').next().unwrap_or("").to_string();

    if is_riot_path(&clean_path) {
        let ok = header("authorization").as_deref() == Some(format!("Bearer {}", access_token()).as_str())
            && header("x-riot-entitlements-jwt").as_deref() == Some(ENT_TOKEN)
            && header("x-riot-clientversion").as_deref() == Some(CLIENT_VERSION)
            && header("x-riot-clientplatform").is_some();
        let (status, body) = if !ok {
            ("401 Unauthorized", String::new())
        } else {
            match riot_route(&method, &clean_path, &String::from_utf8_lossy(&body)) {
                Some(v) => ("200 OK", v.to_string()),
                None => ("404 Not Found", String::new()),
            }
        };
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        s.write_all(resp.as_bytes()).await?;
        return s.shutdown().await;
    }

    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("riot:{PASSWORD}"))
    );
    if header("authorization").as_deref() != Some(expected.as_str()) {
        s.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    }

    if let Some(key) = header("sec-websocket-key") {
        let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
        let resp = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        s.write_all(resp.as_bytes()).await?;
        let mut ws = tokio_tungstenite::WebSocketStream::from_raw_socket(s, Role::Server, None).await;
        while let Some(Ok(msg)) = ws.next().await {
            if let Message::Text(t) = msg {
                if t.contains("OnJsonApiEvent_chat_v4_presences") {
                    let _ = ws.send(Message::Text(String::new())).await;
                    for state in ["PREGAME", "PREGAME", "INGAME"] {
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                        let ev = serde_json::json!([8, "OnJsonApiEvent_chat_v4_presences",
                            { "data": presences(state), "eventType": "Update", "uri": "/chat/v4/presences" }]);
                        let _ = ws.send(Message::Text(ev.to_string())).await;
                    }
                }
            }
        }
        return Ok(());
    }

    let (status, body) = match route(&clean_path) {
        Some(v) => ("200 OK", v.to_string()),
        None => ("404 Not Found", String::new()),
    };
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(resp.as_bytes()).await?;
    s.shutdown().await
}

/// Write a lockfile for the fake server and point the app at it.
pub fn write_lockfile(port: u16, password: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ascend-test-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("lockfile");
    std::fs::write(&path, format!("Riot Client:4242:{port}:{password}:https")).unwrap();
    path
}
