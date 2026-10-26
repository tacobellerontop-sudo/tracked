//! The local half of the connection layer against a fake Riot Client
//! (self-signed TLS on 127.0.0.1, basic auth). No Riot servers involved.

mod common;

use tracked::error::ApiError;
use tracked::riot::presence::{self, LoopState};
use tracked::riot::{self, lockfile, Region, Shard};

#[tokio::test]
async fn connect_against_fake_local_api() {
    let port = common::start().await;
    std::env::set_var("TRACKED_LOCKFILE", common::write_lockfile(port, common::PASSWORD));
    std::env::set_var("TRACKED_CLIENT_VERSION", common::CLIENT_VERSION);

    let lock = lockfile::read().await.expect("lockfile");
    assert_eq!(lock.port, port);

    // M1/M2 (local part): identity, tokens, region, hosts, headers.
    let s = riot::connect().await.expect("connect");
    assert_eq!(s.puuid, common::PUUID);
    assert_eq!(s.riot_id().to_string(), "Tester#0001");
    assert_eq!((s.region, s.shard), (Region::Eu, Shard::Eu));
    assert_eq!(s.hosts.pd, "https://pd.eu.a.pvp.net");
    assert_eq!(s.hosts.glz, "https://glz-eu-1.eu.a.pvp.net");
    let public = s.public().await;
    assert_eq!(public.access_token.expose(), &common::access_token());
    assert_eq!(public.client_version, common::CLIENT_VERSION);
    assert_eq!(s.local_auth().base, format!("https://127.0.0.1:{port}"));

    let p = s.local.presences().await.unwrap();
    let own = presence::own_presence(&p.presences, common::PUUID).unwrap();
    assert_eq!(own.loop_state, LoopState::Menus);
    assert_eq!(own.party_size, 2);
    assert_eq!(s.local.friends().await.unwrap().friends[0].game_name, "Buddy");
}

#[tokio::test]
async fn wrong_password_is_unauthorized() {
    let port = common::start().await;
    let lock = lockfile::parse(&format!("Riot Client:1:{port}:wrong:https")).unwrap();
    let local = tracked::riot::local::LocalClient::new(&lock).unwrap();
    assert!(matches!(local.entitlements().await, Err(ApiError::Unauthorized)));
}

#[tokio::test]
async fn local_tls_refuses_non_ip_hosts() {
    // Same fake server, but addressed by hostname: the loopback-only verifier
    // must refuse the self-signed cert because the target isn't a loopback IP.
    let port = common::start().await;
    let cfg = tracked::riot::tls::local_client_config();
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
