//! Preferences, disclaimer, version.

use super::widgets;
use crate::app::AppState;
use crate::config::{Theme, MAX_POLL_SECS, MIN_POLL_SECS};
use crate::ipc::{Intent, IntentTx, UpdateStatus};

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
            )
            .on_hover_text("How often agent select / in-game rosters refresh. Menus poll much less often.");
            ui.end_row();

            ui.label("Updates");
            ui.checkbox(&mut state.settings.check_for_updates, "Check for updates on startup");
            ui.end_row();
        });

    if state.settings != before {
        state.save_settings(intents);
    }

    ui.add_space(16.0);
    ui.separator();
    ui.label(egui::RichText::new("About").strong());
    ui.horizontal(|ui| {
        ui.label(format!("Ascend-Lite v{}", env!("CARGO_PKG_VERSION")));
        if ui.small_button("Check for updates").clicked() {
            intents.send(Intent::CheckForUpdates);
        }
        match &state.update_status {
            None => {}
            Some(UpdateStatus::Checking) => {
                ui.spinner();
            }
            Some(UpdateStatus::UpToDate) => {
                ui.weak("You're up to date.");
            }
            Some(UpdateStatus::NoReleases) => {
                ui.weak("No releases published yet.");
            }
            Some(UpdateStatus::Available { version, url }) => {
                ui.colored_label(widgets::WIN, format!("v{version} is available"));
                if !url.is_empty() {
                    ui.hyperlink_to("Download", url);
                }
            }
            Some(UpdateStatus::Failed(e)) => {
                ui.weak("Update check failed").on_hover_text(e);
            }
        }
    });
    ui.add_space(6.0);
    ui.label(super::DISCLAIMER);
}
