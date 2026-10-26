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

fn route(path: &str) -> Option<serde_json::Value> {
    Some(match path {
        "/entitlements/v1/token" => serde_json::json!({
            "accessToken": ACCESS_TOKEN, "token": ENT_TOKEN, "subject": PUUID, "entitlements": [], "issuer": "test"
        }),
        "/chat/v1/session" => serde_json::json!({
            "puuid": PUUID, "game_name": "Tester", "game_tag": "0001", "region": "eu1", "loaded": true
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
    let head = String::from_utf8_lossy(&buf).to_string();
    let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
    let header = |name: &str| {
        head.lines().find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim().to_string())
        })
    };
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
                        let ev = serde_json::json!([8, "OnJsonApiEvent_chat_v4_presences",
                            { "data": presences(state), "eventType": "Update", "uri": "/chat/v4/presences" }]);
                        let _ = ws.send(Message::Text(ev.to_string())).await;
                    }
                }
            }
        }
        return Ok(());
    }

    let (status, body) = match route(path.split('?').next().unwrap_or("")) {
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
