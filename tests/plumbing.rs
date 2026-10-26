use std::time::Duration;

use ascend_lite::ipc::{self, Intent, Update};

#[test]
fn ping_round_trips_through_worker() {
    let (tx, rx, sink, updates) = ipc::channels(egui::Context::default());
    let handle = ascend_lite::worker::spawn(rx, sink);
    tx.send(Intent::Ping(42));
    let got = updates
        .recv_timeout(Duration::from_secs(5))
        .expect("no update from worker");
    assert!(matches!(got, Update::Pong(42)));
    tx.send(Intent::Shutdown);
    handle.join().unwrap();
}
