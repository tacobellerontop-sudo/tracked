//! The connection layer's milestones (M1-M4) against a fake Riot Client and
//! fake PD/GLZ on loopback (feature `fake-riot`, so nothing reaches Riot).
//! One sequential test: the steps share process-wide env vars.

mod common;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ascend_lite::ipc::{self, ClientStatus, Intent, Update};
use ascend_lite::riot::{self, pd, GameState, RiotError};
use futures_util::StreamExt;

#[tokio::test(flavor = "multi_thread")]
async fn milestones() {
    std::env::set_var("ASCEND_CLIENT_VERSION", common::CLIENT_VERSION);
    std::env::set_var("ASCEND_OFFLINE_CONTENT", "1");

    // Client not running.
    std::env::set_var("ASCEND_LOCKFILE", "/nonexistent/ascend/lockfile");
    assert_eq!(riot::connect().await.unwrap_err(), RiotError::ClientNotRunning);

    let port = common::start().await;
    let lockfile = common::write_lockfile(port, common::PASSWORD);
    std::env::set_var("ASCEND_LOCKFILE", &lockfile);
    std::env::set_var("ASCEND_FAKE_RIOT_BASE", format!("https://127.0.0.1:{port}"));

    // Signed out: chat session not loaded.
    common::SIGNED_IN.store(false, Ordering::SeqCst);
    assert_eq!(riot::connect().await.unwrap_err(), RiotError::SignedOut);
    common::SIGNED_IN.store(true, Ordering::SeqCst);

    // M1: identity.
    let session = std::sync::Arc::new(riot::connect().await.expect("connect"));
    assert_eq!(session.riot_id().to_string(), "Tester#0001");

    // M2: a real authenticated PD call with the four headers.
    let mmr = pd::player_mmr(&session, &session.puuid).await.expect("PD call");
    assert_eq!(mmr.latest_competitive_update.unwrap().tier_after_update, 10);

    // §8: the token rotates → 401 → refresh from the local API → retry works.
    common::TOKEN_GEN.fetch_add(1, Ordering::SeqCst);
    pd::player_mmr(&session, &session.puuid)
        .await
        .expect("call after token rotation");
    assert_eq!(session.public().await.access_token.expose(), &common::access_token());

    // M3: the state stream: menus, then agent select, then in game, with
    // match ids resolved from GLZ; duplicates collapsed.
    let mut states = Box::pin(riot::watch_state(session.clone()));
    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while seen.len() < 3 && Instant::now() < deadline {
        if let Ok(Some(s)) = tokio::time::timeout(Duration::from_secs(5), states.next()).await {
            seen.push(s);
        }
    }
    assert_eq!(
        seen,
        vec![
            GameState::Menus,
            GameState::PreGame {
                match_id: common::PREGAME_ID.into()
            },
            GameState::InGame {
                match_id: common::CORE_ID.into()
            },
        ]
    );
    drop(states);

    // M4: the full lifecycle in the worker: Ready → client closes (lockfile
    // gone) → Lost → NotRunning → client back → Ready again.
    let (tx, rx, sink, updates) = ipc::channels(egui::Context::default());
    let handle = ascend_lite::worker::spawn(rx, sink, Default::default());
    let wait_status = |want: &dyn Fn(&ClientStatus) -> bool, secs: u64| {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if let Ok(Update::ClientStatus(s)) = updates.recv_timeout(Duration::from_millis(100)) {
                if want(&s) {
                    return s;
                }
            }
        }
        panic!("timed out waiting for status");
    };
    tokio::task::block_in_place(|| {
        wait_status(&|s| matches!(s, ClientStatus::Authed { .. }), 10);
        std::fs::remove_file(&lockfile).unwrap();
        wait_status(&|s| *s == ClientStatus::Lost, 10);
        wait_status(&|s| *s == ClientStatus::NotRunning, 10);
        common::write_lockfile(port, common::PASSWORD);
        let back = wait_status(&|s| matches!(s, ClientStatus::Authed { .. }), 15);
        assert!(matches!(back, ClientStatus::Authed { riot_id: Some(_), .. }));
    });
    tx.send(Intent::Shutdown);
    handle.join().unwrap();
}
