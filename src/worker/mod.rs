//! The worker owns the tokio runtime and runs every piece of I/O.

use std::thread::JoinHandle;

use crate::ipc::{Intent, IntentRx, Update, UpdateSink};

/// Spawn the worker on its own OS thread with a multi-thread tokio runtime.
pub fn spawn(intents: IntentRx, sink: UpdateSink) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("ascend-worker".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("ascend-rt")
                .build()
                .expect("failed to build tokio runtime");
            rt.block_on(run(intents, sink));
        })
        .expect("failed to spawn worker thread")
}

/// The intent dispatch loop.
pub async fn run(mut intents: IntentRx, sink: UpdateSink) {
    tracing::info!("worker started");
    while let Some(intent) = intents.recv().await {
        match intent {
            Intent::Ping(n) => sink.send(Update::Pong(n)),
            Intent::Shutdown => break,
        }
    }
    tracing::info!("worker stopped");
}
