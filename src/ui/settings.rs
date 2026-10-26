//! Preferences, disclaimer, version.

use crate::app::AppState;
use crate::config::{Theme, MAX_POLL_SECS, MIN_POLL_SECS};
use crate::ipc::IntentTx;

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.heading("Settings");
    ui.add_space(8.0);
    let before = state.settings.clone();

    egui::Grid::new("settings")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            ui.label("Theme");
            ui.horizontal(|ui| {
                ui.radio_value(&mut state.settings.theme, Theme::Dark, "Dark");
                ui.radio_value(&mut state.settings.theme, Theme::Light, "Light");
            });
            ui.end_row();

            ui.label("Live polling interval");
            ui.add(
                egui::Slider::new(&mut state.settings.poll_interval_secs, MIN_POLL_SECS..=MAX_POLL_SECS).suffix(" s"),
            );
            ui.end_row();
        });

    if state.settings != before {
        state.save_settings(intents);
    }

    ui.add_space(16.0);
    ui.separator();
    ui.label(egui::RichText::new("About").strong());
    ui.label(format!("Ascend-Lite v{}", env!("CARGO_PKG_VERSION")));
    ui.add_space(6.0);
    ui.label(super::DISCLAIMER);
}
