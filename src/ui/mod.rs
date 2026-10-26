//! Top-level layout: status bar, left nav rail, content panel, toasts.

pub mod career;
pub mod connection;
pub mod crosshairs;
pub mod live;
pub mod lookup;
pub mod match_detail;
pub mod settings;
pub mod social;
pub mod widgets;

use egui::Color32;

use crate::app::{AppState, Screen, Toast};
use crate::backend::models::MinVersionState;
use crate::ipc::{BackendStatus, ClientStatus, Intent, IntentTx};

pub const DISCLAIMER: &str = "Ascend-Lite is an unofficial third-party tool. It is not affiliated with, \
endorsed, sponsored or approved by Riot Games. VALORANT and Riot Games are trademarks of Riot Games, Inc. \
You use this app at your own risk and remain subject to Riot's Terms of Service.\n\n\
Ascend-Lite only uses the Riot Client's local API and Riot's web endpoints with tokens the client issued. \
It never reads game memory, never injects into or modifies any Riot process, and never automates gameplay.";

const NAV: &[(Screen, &str, egui::Key)] = &[
    (Screen::Live, "Live", egui::Key::Num1),
    (Screen::Career, "Career", egui::Key::Num2),
    (Screen::Lookup, "Lookup", egui::Key::Num3),
    (Screen::Crosshairs, "Crosshairs", egui::Key::Num4),
    (Screen::Social, "Social", egui::Key::Num5),
    (Screen::Settings, "Settings", egui::Key::Num6),
    (Screen::Connection, "Connection", egui::Key::Num7),
];

pub fn draw(ctx: &egui::Context, state: &mut AppState, intents: &IntentTx) {
    if !state.settings.disclaimer_accepted {
        egui::CentralPanel::default().show(ctx, |ui| disclaimer(ui, state, intents));
        return;
    }
    if let MinVersionState::Blocked { required, download_url } = state.social.min_version.clone() {
        egui::CentralPanel::default().show(ctx, |ui| update_required(ui, state, intents, &required, download_url));
        return;
    }
    if !state.startup_done {
        toasts(ctx, state);
        connection::startup(ctx, state, intents);
        return;
    }
    let social_on = state.social.status != BackendStatus::Disabled;
    let visible = |s: &Screen| *s != Screen::Social || social_on;

    // Ctrl+<n> switches screens.
    for (screen, _, key) in NAV.iter().filter(|(s, _, _)| visible(s)) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, *key)) {
            state.screen = *screen;
        }
    }

    egui::TopBottomPanel::top("status").show(ctx, |ui| status_bar(ui, state, intents));
    egui::SidePanel::left("nav")
        .resizable(false)
        .exact_width(120.0)
        .show(ctx, |ui| {
            ui.add_space(8.0);
            for (screen, label, _) in NAV.iter().filter(|(s, _, _)| visible(s)) {
                let selected =
                    state.screen == *screen || (state.screen == Screen::MatchDetail && state.return_to == *screen);
                let resp = ui.add_sized(
                    [ui.available_width(), 32.0],
                    egui::SelectableLabel::new(selected, *label),
                );
                if resp.clicked() {
                    state.screen = *screen;
                }
            }
        });
    toasts(ctx, state);
    egui::CentralPanel::default().show(ctx, |ui| {
        // Social manages its own scroll areas (friends, conversations, messages).
        if state.screen == Screen::Social {
            social::ui(ui, state, intents);
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match state.screen {
                Screen::Live => live::ui(ui, state, intents),
                Screen::Career => career::ui(ui, state, intents),
                Screen::Lookup => lookup::ui(ui, state, intents),
                Screen::Crosshairs => crosshairs::ui(ui, state, intents),
                Screen::MatchDetail => match_detail::ui(ui, state, intents),
                Screen::Settings => settings::ui(ui, state, intents),
                Screen::Connection => connection::ui(ui, state, intents),
                Screen::Social => {}
            });
    });
}

/// The backend says this version is too old: lock the app (version gate).
fn update_required(
    ui: &mut egui::Ui,
    state: &mut AppState,
    intents: &IntentTx,
    required: &str,
    download_url: Option<String>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        ui.heading("Update required");
        ui.add_space(8.0);
        ui.label(format!(
            "The configured backend requires Ascend-Lite v{required} or newer. You're running v{}.",
            env!("CARGO_PKG_VERSION")
        ));
        if let Some(url) = download_url {
            ui.hyperlink_to("Download the latest version", url);
        }
        ui.add_space(16.0);
        if ui
            .button("Turn off social features and keep using the tracker")
            .clicked()
        {
            state.settings.backend_url = None;
            state.social.url_input.clear();
            state.save_settings(intents);
            intents.send(Intent::BackendConfigure);
        }
    });
}

fn status_bar(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.horizontal(|ui| {
        let (color, text) = match &state.status {
            ClientStatus::NotRunning => (Color32::from_rgb(200, 70, 70), "VALORANT isn't running".to_string()),
            ClientStatus::Connecting(m) => (Color32::from_rgb(220, 170, 60), m.clone()),
            ClientStatus::Authed { riot_id, region } => (
                Color32::from_rgb(80, 190, 110),
                match riot_id {
                    Some(id) => format!("{id} · {}", region.to_uppercase()),
                    None => format!("Connected · {}", region.to_uppercase()),
                },
            ),
        };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.5, color);
        let link = ui
            .add(egui::Button::new(text).frame(false))
            .on_hover_text("Connection details");
        if link.clicked() {
            state.screen = Screen::Connection;
        }
        if let Some(g) = &state.game {
            ui.separator();
            ui.label(live::loop_state_label(g.loop_state));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !matches!(state.status, ClientStatus::Authed { .. }) && ui.small_button("Retry now").clicked() {
                intents.send(Intent::Bootstrap);
            }
        });
    });
}

fn toasts(ctx: &egui::Context, state: &mut AppState) {
    let now = ctx.input(|i| i.time);
    for text in state.pending_toasts.drain(..) {
        if !state.toasts.iter().any(|t| t.text == text) {
            state.toasts.push(Toast { text, until: now + 6.0 });
        }
    }
    state.toasts.retain(|t| t.until > now);
    if state.toasts.is_empty() {
        return;
    }
    egui::TopBottomPanel::bottom("toasts").show(ctx, |ui| {
        for t in &state.toasts {
            ui.colored_label(Color32::from_rgb(230, 120, 100), &t.text);
        }
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}

fn disclaimer(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    let margin = ((ui.available_width() - 560.0) / 2.0).max(16.0);
    ui.horizontal(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_max_width(560.0);
            disclaimer_body(ui, state, intents);
        });
    });
}

fn disclaimer_body(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.vertical(|ui| {
        ui.add_space(60.0);
        ui.heading("Before you start");
        ui.add_space(12.0);
        ui.label(DISCLAIMER);
        ui.add_space(16.0);
        if ui.button("I understand — continue").clicked() {
            state.settings.disclaimer_accepted = true;
            state.save_settings(intents);
        }
    });
}
