use std::time::{Duration, Instant};

use ascend_lite::ipc::{self, Intent, Update};

#[test]
fn ping_round_trips_through_worker() {
    std::env::set_var("ASCEND_LOCKFILE", "/nonexistent/ascend-test/lockfile");
    let (tx, rx, sink, updates) = ipc::channels(egui::Context::default());
    let handle = ascend_lite::worker::spawn(rx, sink, Default::default());
    tx.send(Intent::Ping(42));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_status = false;
    let mut saw_pong = false;
    while Instant::now() < deadline && !(saw_pong && saw_status) {
        match updates.recv_timeout(Duration::from_millis(100)) {
            Ok(Update::Pong(42)) => saw_pong = true,
            Ok(Update::ClientStatus(ipc::ClientStatus::NotRunning)) => saw_status = true,
            _ => {}
        }
    }
    assert!(saw_pong, "no pong from worker");
    assert!(saw_status, "worker should report VALORANT not running");
    tx.send(Intent::Shutdown);
    handle.join().unwrap();
}
