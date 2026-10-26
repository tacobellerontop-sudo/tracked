//! `eframe::App` implementation and the UI-thread state.

use crate::ipc::{Intent, IntentTx, Update, UpdateRx};

/// Plain data owned by the UI thread. Mutated only from [`Update`]s and
/// direct user input.
#[derive(Default)]
pub struct AppState {
    pub pings_sent: u64,
    pub last_pong: Option<u64>,
}

impl AppState {
    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Pong(n) => self.last_pong = Some(n),
        }
    }
}

pub struct App {
    pub state: AppState,
    intents: IntentTx,
    updates: UpdateRx,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl App {
    pub fn new(intents: IntentTx, updates: UpdateRx, worker: std::thread::JoinHandle<()>) -> Self {
        Self {
            state: AppState::default(),
            intents,
            updates,
            worker: Some(worker),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(update) = self.updates.try_recv() {
            self.state.apply(update);
        }
        crate::ui::draw(ctx, &mut self.state, &self.intents);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.intents.send(Intent::Shutdown);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}
