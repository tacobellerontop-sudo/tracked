mod common;

use std::time::Duration;

use ascend_lite::error::ApiError;
use ascend_lite::riot::lockfile;
use ascend_lite::riot::presence::{self, LoopState, PresenceEvent};
use ascend_lite::riot::Session;

#[tokio::test]
async fn bootstrap_against_fake_local_api() {
    let port = common::start().await;
    let path = common::write_lockfile(port, common::PASSWORD);
    std::env::set_var("ASCEND_LOCKFILE", &path);

    let lock = lockfile::read().await.expect("lockfile");
    assert_eq!(lock.port, port);

    let session = Session::connect(&lock, reqwest::Client::new(), "release-test".into())
        .await
        .expect("connect");
    assert_eq!(session.puuid, common::PUUID);
    assert_eq!(session.region.region, "eu");
    assert_eq!(session.region.shard, "eu");
    assert_eq!(session.riot_id.as_ref().unwrap().to_string(), "Tester#0001");

    let p = session.local.presences().await.unwrap();
    let own = presence::own_presence(&p.presences, common::PUUID).unwrap();
    assert_eq!(own.loop_state, LoopState::Menus);
    assert_eq!(own.party_size, 2);

    let friends = session.local.friends().await.unwrap();
    assert_eq!(friends.friends[0].game_name, "Buddy");

    // Presence websocket: we should see PREGAME then INGAME (duplicates collapsed).
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(presence::run_socket(
        port,
        lock.password.clone(),
        common::PUUID.into(),
        tx,
    ));
    let mut states = Vec::new();
    while states.len() < 2 {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("socket timeout")
        {
            Some(PresenceEvent::Changed(g)) => states.push(g.loop_state),
            Some(_) => {}
            None => break,
        }
    }
    assert_eq!(states, vec![LoopState::Pregame, LoopState::InGame]);
}

#[tokio::test]
async fn wrong_password_is_unauthorized() {
    let port = common::start().await;
    let lock = lockfile::parse(&format!("Riot Client:1:{port}:wrong:https")).unwrap();
    let local = ascend_lite::riot::local::LocalClient::new(&lock).unwrap();
    assert!(matches!(local.entitlements().await, Err(ApiError::Unauthorized)));
}

#[tokio::test]
async fn local_tls_refuses_non_ip_hosts() {
    // Same fake server, but addressed by hostname: the loopback-only verifier
    // must refuse the self-signed cert because the target isn't a loopback IP.
    let port = common::start().await;
    let cfg = ascend_lite::riot::tls::local_client_config();
    let http = reqwest::Client::builder()
        .use_preconfigured_tls((*cfg).clone())
        .no_proxy()
        .build()
        .unwrap();
    let ok = http.get(format!("https://127.0.0.1:{port}/x")).send().await;
    assert!(ok.is_ok(), "loopback IP should be accepted");
    let err = http.get(format!("https://localhost:{port}/x")).send().await;
    assert!(err.is_err(), "hostname must be rejected");
}
