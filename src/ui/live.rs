//! Live game state.

use egui::RichText;

use crate::app::AppState;
use crate::ipc::{ClientStatus, IntentTx};
use crate::riot::presence::LoopState;

pub fn loop_state_label(s: LoopState) -> &'static str {
    match s {
        LoopState::Menus => "In menus",
        LoopState::Pregame => "Agent select",
        LoopState::InGame => "In game",
        LoopState::Unknown => "Unknown state",
    }
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, _intents: &IntentTx) {
    ui.heading("Live");
    ui.add_space(6.0);
    if !matches!(state.status, ClientStatus::Authed { .. }) {
        ui.label("Start VALORANT to see live match information.");
        return;
    }
    match &state.game {
        None => {
            ui.label("Waiting for game state…");
        }
        Some(g) => {
            ui.label(RichText::new(loop_state_label(g.loop_state)).size(20.0).strong());
            let mut bits = Vec::new();
            if !g.queue_id.is_empty() {
                bits.push(crate::content::valorant_api::queue_name(&g.queue_id).to_string());
            }
            if !g.map.is_empty() {
                bits.push(state.catalog.map_name(&g.map));
            }
            if g.party_size > 0 {
                bits.push(format!("party of {}", g.party_size));
            }
            if !bits.is_empty() {
                ui.weak(bits.join(" · "));
            }
        }
    }
}
