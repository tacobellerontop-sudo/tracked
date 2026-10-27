//! Preferences, disclaimer, version.

use super::widgets;
use crate::app::AppState;
use crate::config::{RendererPref, Theme, MAX_POLL_SECS, MIN_POLL_SECS};
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

            ui.label("Region");
            region_picker(ui, state);
            ui.end_row();

            ui.label("Animations");
            ui.checkbox(&mut state.settings.animations, "Smooth transitions and motion");
            ui.end_row();

            ui.label("Renderer");
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let wgpu = state.settings.renderer.uses_wgpu();
                    if ui
                        .radio(!wgpu, "OpenGL (recommended)")
                        .on_hover_text("Runs on your GPU. Falls back to Direct3D 12 / Vulkan if it can't start.")
                        .clicked()
                    {
                        state.settings.renderer = RendererPref::Auto;
                    }
                    if ui
                        .radio(wgpu, "Direct3D 12 / Vulkan (experimental)")
                        .on_hover_text("Uses wgpu. Can feel laggy on some PCs; falls back to OpenGL if it can't start.")
                        .clicked()
                    {
                        state.settings.renderer = RendererPref::Wgpu;
                    }
                });
                ui.label(
                    egui::RichText::new(format!(
                        "Now: {} · UI work {:.1} ms per frame · changes apply after a restart",
                        state.renderer, state.frame_ms
                    ))
                    .small()
                    .weak(),
                );
            });
            ui.end_row();

            ui.label("Window");
            ui.checkbox(&mut state.settings.system_title_bar, "Use the system title bar")
                .on_hover_text("Takes effect after a restart.");
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
    social_settings(ui, state, intents);

    ui.add_space(16.0);
    ui.separator();
    replay_files(ui, state, intents);

    ui.add_space(16.0);
    ui.separator();
    ui.label(egui::RichText::new("About").strong());
    ui.horizontal(|ui| {
        ui.label(format!("Tracked v{}", env!("CARGO_PKG_VERSION")));
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

fn social_settings(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.label(egui::RichText::new("Tracked backend (optional)").strong());
    ui.weak(
        "Your Riot friends and whispers work without this (Social tab). A backend adds Tracked-only \
         friends, lobbies and DMs. Riot credentials are never sent to the backend.",
    );
    ui.horizontal(|ui| {
        ui.label("Backend URL");
        ui.add(
            egui::TextEdit::singleline(&mut state.social.url_input)
                .hint_text("https://… (or http://127.0.0.1:8787 for the mock)")
                .desired_width(320.0),
        );
        let input = state.social.url_input.trim().to_string();
        let current = state.settings.backend_url.clone().unwrap_or_default();
        if ui.add_enabled(input != current, egui::Button::new("Apply")).clicked() {
            if input.is_empty() {
                apply_url(state, intents, None);
            } else {
                match crate::backend::client::validate_base(&input) {
                    Ok(_) => apply_url(state, intents, Some(input)),
                    Err(e) => state.pending_toasts.push(crate::error::UserFacingError::from(&e).0),
                }
            }
        }
    });
    let before = (state.settings.share_presence, state.settings.share_lobby);
    ui.checkbox(
        &mut state.settings.share_presence,
        "Share my game status (menus / agent select / in game) with friends",
    );
    ui.checkbox(
        &mut state.settings.share_lobby,
        "Share my party size and queue with friends",
    );
    if (state.settings.share_presence, state.settings.share_lobby) != before {
        state.save_settings(intents);
    }
    if state.social.me().is_some() {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.social.feedback)
                    .hint_text("Feedback")
                    .desired_width(320.0),
            );
            if ui
                .add_enabled(
                    !state.social.feedback.trim().is_empty(),
                    egui::Button::new("Send feedback"),
                )
                .clicked()
            {
                intents.send(Intent::SendFeedback(state.social.feedback.trim().to_string()));
                state.social.feedback.clear();
            }
        });
    }
}

fn apply_url(state: &mut AppState, intents: &IntentTx, url: Option<String>) {
    state.settings.backend_url = url;
    state.save_settings(intents);
    intents.send(Intent::BackendConfigure);
}

/// Region override: automatic detection, or a fixed VALORANT region.
fn region_picker(ui: &mut egui::Ui, state: &mut AppState) {
    use crate::riot::region::Region;
    let current = match &state.status {
        crate::ipc::ClientStatus::Authed { region, .. } => Some(region.to_uppercase()),
        _ => None,
    };
    let automatic = state.settings.region_override.is_none();
    let label = |code: Option<&str>| match code {
        None => match &current {
            Some(r) if automatic => format!("Automatic (using {r})"),
            _ => "Automatic".to_string(),
        },
        Some(c) => region_name(c).to_string(),
    };
    ui.vertical(|ui| {
        let selected = label(state.settings.region_override.as_deref());
        egui::ComboBox::from_id_salt("region-override")
            .selected_text(selected)
            .width(220.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.settings.region_override, None, label(None));
                for r in Region::ALL {
                    let code = Some(r.as_str().to_string());
                    ui.selectable_value(&mut state.settings.region_override, code, region_name(r.as_str()));
                }
            });
        ui.label(
            egui::RichText::new(
                "Pick yours if Tracked shows the wrong region (e.g. Unranked with no matches). Reconnects right away.",
            )
            .small()
            .weak(),
        );
    });
}

fn region_name(code: &str) -> &'static str {
    match code {
        "na" => "North America (NA)",
        "latam" => "Latin America (LATAM)",
        "br" => "Brazil (BR)",
        "eu" => "Europe (EU)",
        "ap" => "Asia Pacific (AP)",
        "kr" => "Korea (KR)",
        _ => "Unknown",
    }
}

/// Inspect VALORANT's downloaded replay files (structure only), so exact
/// movement can be decoded from them later.
fn replay_files(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.label(egui::RichText::new("Replay files").strong());
    ui.weak(
        "Checks the replays VALORANT has downloaded (Saved\\Demos) and reports how the files are laid out. \
         It reads the files only, never the game, and shows no player names or positions.",
    );
    ui.horizontal(|ui| {
        if ui.button("Check replay files").clicked() {
            intents.send(Intent::CheckReplayFiles);
            // Empty = running; the worker replies with the report.
            state.replay_file_report = Some(String::new());
        }
        if let Some(report) = state.replay_file_report.as_deref().filter(|r| !r.is_empty()) {
            if ui.button("Copy report").clicked() {
                ui.ctx().copy_text(report.to_string());
            }
        }
    });
    match state.replay_file_report.as_deref() {
        Some("") => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Reading replay files…");
            });
        }
        Some(report) => {
            egui::ScrollArea::vertical()
                .id_salt("replay-file-report")
                .max_height(220.0)
                .show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(report).monospace().size(11.0)).wrap());
                });
        }
        None => {}
    }
}
