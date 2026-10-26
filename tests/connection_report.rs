//! The startup checklist reports the right step when connecting fails.

mod common;

use std::time::{Duration, Instant};

use tracked::diagnostics::{ConnectionReport, Step, StepState};
use tracked::ipc::{self, Intent, Update};

fn run_until(lockfile: std::path::PathBuf, done: impl Fn(&ConnectionReport) -> bool) -> ConnectionReport {
    std::env::set_var("TRACKED_LOCKFILE", lockfile);
    std::env::set_var("TRACKED_OFFLINE_CONTENT", "1");
    std::env::set_var("TRACKED_CLIENT_VERSION", common::CLIENT_VERSION);
    let (tx, rx, sink, updates) = ipc::channels(egui::Context::default());
    let handle = tracked::worker::spawn(rx, sink, Default::default());
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last = ConnectionReport::default();
    while Instant::now() < deadline {
        if let Ok(Update::Connection(r)) = updates.recv_timeout(Duration::from_millis(100)) {
            last = r;
            if done(&last) {
                break;
            }
        }
    }
    tx.send(Intent::Shutdown);
    handle.join().unwrap();
    last
}

#[test]
fn missing_client_and_wrong_password_are_reported() {
    // Both cases in one test: they share process-wide env vars.
    let r = run_until("/nonexistent/ascend/lockfile".into(), |r| r.failure().is_some());
    let (step, msg, hint) = r.failure().expect("a failed step");
    assert_eq!(step, Step::Lockfile);
    assert!(msg.contains("isn't running"), "{msg}");
    assert!(hint.contains("Start the Riot Client"), "{hint}");
    assert!(!r.connected());

    let rt = tokio::runtime::Runtime::new().unwrap();
    let port = rt.block_on(common::start());
    let lock = common::write_lockfile(port, "not-the-password");
    let r = run_until(lock, |r| r.failure().is_some_and(|f| f.0 != Step::Lockfile));
    assert!(matches!(r.state(Step::Lockfile), StepState::Ok(_)));
    let (step, msg, _) = r.failure().expect("a failed step");
    assert_eq!(step, Step::LocalApi);
    assert!(msg.contains("rejected the lockfile password"), "{msg}");
    assert!(!msg.contains("not-the-password"), "never echo the password");
}
