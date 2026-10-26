//! Top-level layout.

use crate::app::AppState;
use crate::ipc::{Intent, IntentTx};

pub fn draw(ctx: &egui::Context, state: &mut AppState, intents: &IntentTx) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Ascend-Lite — plumbing check");
        if ui.button("Ping worker").clicked() {
            state.pings_sent += 1;
            intents.send(Intent::Ping(state.pings_sent));
        }
        ui.label(format!("pings sent: {}", state.pings_sent));
        match state.last_pong {
            Some(n) => ui.label(format!("last pong: {n}")),
            None => ui.label("no pong yet"),
        };
    });
}
