//! Run the credential-bearing code paths with the app's most verbose log
//! filter (`--debug`, plus TRACE for our own crate) and make sure no sentinel
//! secret ever reaches the log output.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ascend_lite::riot::{lockfile, presence, Session};
use base64::Engine;

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
    let filter = ascend_lite::logging::filter(true).add_directive("ascend_lite=trace".parse().unwrap());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(move || writer.clone())
        .init();

    let port = common::start().await;
    std::env::set_var("ASCEND_LOCKFILE", common::write_lockfile(port, common::PASSWORD));

    let lock = lockfile::read().await.unwrap();
    tracing::debug!(target: "ascend_lite", ?lock, "lockfile loaded");
    let session = Session::connect(&lock, reqwest::Client::new(), "release-test".into())
        .await
        .unwrap();
    tracing::debug!(target: "ascend_lite", ?session, "session established");
    let _ = session.local.presences().await;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(presence::run_socket(
        port,
        lock.password.clone(),
        common::PUUID.into(),
        tx,
    ));
    let _ = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await;

    // Also exercise the error path with a bad password.
    let bad = lockfile::parse(&format!("Riot Client:1:{port}:{}X:https", common::PASSWORD)).unwrap();
    let err = Session::connect(&bad, reqwest::Client::new(), "v".into())
        .await
        .unwrap_err();
    tracing::warn!(target: "ascend_lite", "expected failure: {err} / {err:?}");

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
