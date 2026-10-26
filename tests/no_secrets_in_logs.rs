//! Run the credential-bearing code paths with the app's most verbose log
//! filter (`--debug`, plus TRACE for our own crate) and make sure no sentinel
//! secret ever reaches the log output.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use futures_util::StreamExt;
use tracked::riot::{self, lockfile};

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

#[tokio::test(flavor = "multi_thread")]
async fn sample_run_never_logs_secrets() {
    let buf = Buf::default();
    let writer = buf.clone();
    let filter = tracked::logging::filter(true).add_directive("tracked=trace".parse().unwrap());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(move || writer.clone())
        .init();

    let port = common::start().await;
    std::env::set_var("TRACKED_LOCKFILE", common::write_lockfile(port, common::PASSWORD));
    std::env::set_var("TRACKED_CLIENT_VERSION", common::CLIENT_VERSION);
    std::env::set_var("TRACKED_FAKE_RIOT_BASE", format!("https://127.0.0.1:{port}"));

    let lock = lockfile::read().await.unwrap();
    tracing::debug!(target: "tracked", ?lock, "lockfile loaded");
    let session = std::sync::Arc::new(riot::connect().await.unwrap());
    tracing::debug!(target: "tracked", ?session, "session established");
    tracing::debug!(target: "tracked", public = ?session.public().await, auth = ?session.local_auth(), "auth");
    let _ = session.local.presences().await;
    let _ = tracked::riot::pd::player_mmr(&session, &session.puuid).await;

    // The websocket (its handshake carries the basic-auth header).
    let mut events = Box::pin(riot::watch(session.clone()));
    for _ in 0..3 {
        let _ = tokio::time::timeout(Duration::from_secs(3), events.next()).await;
    }
    drop(events);

    // Also exercise the error path with a bad password.
    std::env::set_var(
        "TRACKED_LOCKFILE",
        common::write_lockfile(port, &format!("{}X", common::PASSWORD)),
    );
    let err = riot::connect().await.unwrap_err();
    tracing::warn!(target: "tracked", "expected failure: {err} / {err:?}");

    let logs = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("session established"), "logging was not captured");
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("riot:{}", common::PASSWORD));
    for secret in [
        common::PASSWORD,
        common::ACCESS_TOKEN,
        common::ENT_TOKEN,
        basic.as_str(),
    ] {
        assert!(!logs.contains(secret), "secret leaked into logs: {secret}\n{logs}");
    }
}
