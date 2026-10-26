//! The contract between the UI thread and the worker.

use std::sync::mpsc;

/// Sent UI -> worker. One variant per user action / background need.
#[derive(Debug, Clone)]
pub enum Intent {
    /// Plumbing check: the worker answers with [`Update::Pong`].
    Ping(u64),
    Shutdown,
}

/// Sent worker -> UI. One variant per thing the UI can display.
#[derive(Debug, Clone)]
pub enum Update {
    Pong(u64),
}

/// UI-side handle for sending intents. Never blocks.
#[derive(Clone)]
pub struct IntentTx(tokio::sync::mpsc::UnboundedSender<Intent>);

impl IntentTx {
    pub fn send(&self, intent: Intent) {
        // The worker only goes away at shutdown; dropping the intent then is fine.
        let _ = self.0.send(intent);
    }
}

pub type IntentRx = tokio::sync::mpsc::UnboundedReceiver<Intent>;

/// Worker-side handle for sending updates. Wakes the UI after every push.
#[derive(Clone)]
pub struct UpdateSink {
    tx: mpsc::Sender<Update>,
    ctx: egui::Context,
}

impl UpdateSink {
    pub fn send(&self, update: Update) {
        if self.tx.send(update).is_ok() {
            self.ctx.request_repaint();
        }
    }
}

pub type UpdateRx = mpsc::Receiver<Update>;

/// Build both channels. `ctx` is used to wake the UI when updates arrive.
pub fn channels(ctx: egui::Context) -> (IntentTx, IntentRx, UpdateSink, UpdateRx) {
    let (itx, irx) = tokio::sync::mpsc::unbounded_channel();
    let (utx, urx) = mpsc::channel();
    (IntentTx(itx), irx, UpdateSink { tx: utx, ctx }, urx)
}
