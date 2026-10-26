//! End-to-end: the real worker against a fake Riot client + fake PD/GLZ on
//! loopback (feature `fake-riot`). Walks the Phase 1+2 gates:
//! bootstrap -> career -> presence says agent select / in game -> roster with
//! ranks for every player — and checks streamer-mode privacy and log hygiene.
//!
//! Run with: `cargo test --features fake-riot --test e2e_worker`

mod common;

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ascend_lite::config::Settings;
use ascend_lite::ipc::{self, ClientStatus, Intent, Update};
use ascend_lite::riot::models::LivePhase;

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn worker_end_to_end() {
    let logs = Buf::default();
    let writer = logs.clone();
    let filter = ascend_lite::logging::filter(true).add_directive("ascend_lite=trace".parse().unwrap());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(move || writer.clone())
        .init();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let port = rt.block_on(common::start());
    let tmp = std::env::temp_dir().join(format!("ascend-e2e-{}", std::process::id()));
    std::env::set_var("ASCEND_LOCKFILE", common::write_lockfile(port, common::PASSWORD));
    std::env::set_var("ASCEND_CLIENT_VERSION", common::CLIENT_VERSION);
    std::env::set_var("ASCEND_FAKE_RIOT_BASE", format!("https://127.0.0.1:{port}"));
    std::env::set_var("ASCEND_CACHE_DIR", tmp.join("cache"));
    std::env::set_var("ASCEND_CONFIG_DIR", tmp.join("cfg"));

    let (tx, rx, sink, updates) = ipc::channels(egui::Context::default());
    let settings = Settings {
        check_for_updates: false,
        poll_interval_secs: 3,
        ..Default::default()
    };
    let handle = ascend_lite::worker::spawn(rx, sink, settings);

    let mut authed = false;
    let mut career = None;
    let mut match_loaded = false;
    let mut live = None;
    let mut probes: HashMap<String, u32> = HashMap::new();
    let mut names: HashMap<String, String> = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    let done = |live: &Option<ascend_lite::riot::models::LiveMatch>, probes: &HashMap<String, u32>| {
        live.as_ref()
            .is_some_and(|m| m.phase == LivePhase::InGame && probes.len() == 10)
    };
    while Instant::now() < deadline && !(career.is_some() && match_loaded && done(&live, &probes)) {
        let Ok(u) = updates.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        match u {
            Update::ClientStatus(ClientStatus::Authed { riot_id, region }) => {
                assert_eq!(region, "eu");
                assert_eq!(riot_id.unwrap().to_string(), "Tester#0001");
                authed = true;
            }
            Update::Career(c) => career = Some(c),
            Update::MatchDetails(id, _) if id == common::MATCH_ID => match_loaded = true,
            Update::LiveMatch(m) => live = m,
            Update::Probe(p, Ok(pr)) => {
                probes.insert(p, pr.rank.tier);
            }
            Update::Names(n) => names.extend(n.into_iter().map(|(p, id)| (p, id.to_string()))),
            _ => {}
        }
    }

    assert!(authed, "never authenticated");
    let career = career.expect("no career data");
    assert_eq!(career.rank.tier, 10, "self is player 0 -> tier 10");
    assert_eq!(career.rank.rr, 42);
    assert_eq!(career.rank.season_games, 20);
    assert_eq!(career.rank.peak_tier, 21);
    assert_eq!(career.rr_history.len(), 2);
    assert!(match_loaded, "match details never arrived");

    let live = live.expect("no live match");
    assert_eq!(live.phase, LivePhase::InGame);
    assert_eq!(
        live.teams(),
        vec!["Blue".to_string(), "Red".to_string()],
        "own team first"
    );
    assert_eq!(live.players.len(), 10);
    for i in 0..10 {
        assert_eq!(
            probes.get(&common::player(i)),
            Some(&(10 + i as u32)),
            "rank for player {i}"
        );
    }

    // Streamer mode: hidden player's name is never requested or shown.
    let hidden = common::player(common::INCOGNITO);
    assert!(live.players.iter().find(|p| p.puuid == hidden).unwrap().incognito);
    assert!(live
        .players
        .iter()
        .find(|p| p.puuid == hidden)
        .unwrap()
        .account_level
        .is_none());
    assert!(!names.contains_key(&hidden), "incognito name leaked to UI");
    assert!(
        !common::NAME_REQUESTS.lock().unwrap().contains(&hidden),
        "incognito name was requested"
    );
    assert_eq!(names.get(&common::player(5)).map(String::as_str), Some("Player5#TAG"));

    tx.send(Intent::Shutdown);
    handle.join().unwrap();

    let logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("riot session established"), "logs not captured");
    for secret in [common::PASSWORD, common::ACCESS_TOKEN, common::ENT_TOKEN] {
        assert!(!logs.contains(secret), "secret leaked into logs: {secret}");
    }
    let cache = std::fs::read_dir(tmp.join("cache")).map(|d| d.count()).unwrap_or(0);
    assert!(cache > 0, "match cache should be written");
    for entry in walk(&tmp) {
        let content = std::fs::read(&entry).unwrap_or_default();
        let text = String::from_utf8_lossy(&content);
        for secret in [common::PASSWORD, common::ACCESS_TOKEN, common::ENT_TOKEN] {
            assert!(!text.contains(secret), "secret written to disk in {}", entry.display());
        }
    }
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}
