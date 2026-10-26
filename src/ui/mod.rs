//! Top-level layout: sidebar, top bar (search + theme), content, toasts.

pub mod career;
pub mod connection;
pub mod crosshairs;
pub mod home;
pub mod icons;
pub mod live;
pub mod lookup;
pub mod match_detail;
pub mod pages;
pub mod settings;
pub mod social;
pub mod theme;
pub mod widgets;

use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Shape, Stroke};

use crate::app::{AppState, Screen, Toast};
use crate::backend::models::MinVersionState;
use crate::config::Theme;
use crate::ipc::{BackendStatus, ClientStatus, Intent, IntentTx};
use icons::Icon;
use theme::pal;

pub const DISCLAIMER: &str = "Ascend-Lite is an unofficial third-party tool. It is not affiliated with, \
endorsed, sponsored or approved by Riot Games. VALORANT and Riot Games are trademarks of Riot Games, Inc. \
You use this app at your own risk and remain subject to Riot's Terms of Service.\n\n\
Ascend-Lite only uses the Riot Client's local API and Riot's web endpoints with tokens the client issued. \
It never reads game memory, never injects into or modifies any Riot process, and never automates gameplay.";

const NAV: &[(Screen, &str, Icon, egui::Key)] = &[
    (Screen::Home, "Home", Icon::Home, egui::Key::Num1),
    (Screen::Live, "Live", Icon::Live, egui::Key::Num2),
    (Screen::Matches, "Matches", Icon::Matches, egui::Key::Num3),
    (Screen::Agents, "Agents", Icon::Agents, egui::Key::Num4),
    (Screen::Lookup, "Players", Icon::Search, egui::Key::Num5),
    (Screen::Crosshairs, "Crosshairs", Icon::Crosshair, egui::Key::Num6),
    (Screen::Social, "Social", Icon::Social, egui::Key::Num7),
    (Screen::Settings, "Settings", Icon::Settings, egui::Key::Num8),
];

const SIDEBAR_W: f32 = 210.0;

pub fn draw(ctx: &egui::Context, state: &mut AppState, intents: &IntentTx) {
    if !state.settings.disclaimer_accepted {
        egui::CentralPanel::default().show(ctx, |ui| disclaimer(ui, state, intents));
        return;
    }
    if let MinVersionState::Blocked { required, download_url } = state.social.min_version.clone() {
        egui::CentralPanel::default().show(ctx, |ui| update_required(ui, state, intents, &required, download_url));
        return;
    }
    toasts(ctx, state);
    if !state.startup_done {
        connection::startup(ctx, state, intents);
        return;
    }
    let social_on = state.social.status != BackendStatus::Disabled;
    let visible = |s: &Screen| *s != Screen::Social || social_on;

    // Ctrl+<n> switches screens.
    for (screen, _, _, key) in NAV.iter().filter(|n| visible(&n.0)) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, *key)) {
            state.screen = *screen;
        }
    }

    let p = pal(ctx);
    egui::SidePanel::left("nav")
        .resizable(false)
        .exact_width(SIDEBAR_W)
        .frame(egui::Frame::NONE.fill(p.sidebar).inner_margin(egui::Margin {
            left: 16,
            right: 16,
            top: 22,
            bottom: 16,
        }))
        .show(ctx, |ui| sidebar(ui, state, &visible));
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(p.bg).inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: 18,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            top_bar(ui, state, intents);
            ui.add_space(14.0);
            // Social manages its own scroll areas (friends, conversations, messages).
            if state.screen == Screen::Social {
                theme::card(ui, None, |ui| social::ui(ui, state, intents));
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 16.0;
                match state.screen {
                    Screen::Home => home::ui(ui, state, intents),
                    Screen::Matches => pages::matches(ui, state, intents),
                    Screen::Agents => pages::agents(ui, state),
                    Screen::Live => theme::card(ui, None, |ui| live::ui(ui, state, intents)),
                    Screen::Lookup => theme::card(ui, None, |ui| lookup::ui(ui, state, intents)),
                    Screen::Crosshairs => theme::card(ui, None, |ui| crosshairs::ui(ui, state, intents)),
                    Screen::MatchDetail => theme::card(ui, None, |ui| match_detail::ui(ui, state, intents)),
                    Screen::Settings => theme::card(ui, None, |ui| settings::ui(ui, state, intents)),
                    Screen::Connection => theme::card(ui, None, |ui| connection::ui(ui, state, intents)),
                    Screen::Social => {}
                }
                ui.add_space(24.0);
            });
        });
}

fn sidebar(ui: &mut egui::Ui, state: &mut AppState, visible: &dyn Fn(&Screen) -> bool) {
    let p = pal(ui.ctx());
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
        icons::logo(ui.painter(), r, p.accent);
        ui.add_space(4.0);
        ui.label(RichText::new("A S C E N D").size(13.0).color(p.text));
    });
    ui.add_space(30.0);

    for (screen, label, icon, _) in NAV.iter().filter(|n| visible(&n.0)) {
        let selected = state.screen == *screen || (state.screen == Screen::MatchDetail && state.return_to == *screen);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
        if selected || resp.hovered() {
            let fill = if selected {
                p.selected
            } else {
                p.selected.gamma_multiply(0.5)
            };
            ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
        }
        if selected {
            let bar = Rect::from_min_size(rect.left_top() + vec2(0.0, 10.0), vec2(3.0, rect.height() - 20.0));
            ui.painter().rect_filled(bar, CornerRadius::same(2), p.accent);
        }
        let color = if selected { p.text } else { p.weak };
        let ic = Rect::from_center_size(rect.left_center() + vec2(24.0, 0.0), vec2(18.0, 18.0));
        icons::paint(ui.painter(), ic, color, *icon);
        ui.painter().text(
            rect.left_center() + vec2(48.0, 0.0),
            Align2::LEFT_CENTER,
            *label,
            FontId::proportional(13.5),
            color,
        );
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            state.screen = *screen;
        }
        ui.add_space(2.0);
    }

    // Bottom: tagline, decorative shards, profile chip.
    let bottom = ui.max_rect().bottom();
    let width = ui.available_width();
    let deco = Rect::from_min_max(
        pos2(ui.max_rect().left() - 16.0, bottom - 260.0),
        pos2(ui.max_rect().right() + 16.0, bottom - 60.0),
    );
    if deco.top() > ui.cursor().top() + 20.0 {
        let painter = ui.painter();
        let at = |x: f32, y: f32| pos2(deco.left() + deco.width() * x, deco.top() + deco.height() * y);
        for (pts, a) in [
            (vec![at(0.0, 0.55), at(0.38, 0.30), at(0.22, 1.0)], 0.18),
            (vec![at(0.0, 0.85), at(0.55, 0.62), at(0.40, 1.0)], 0.30),
            (vec![at(0.55, 0.75), at(0.80, 0.55), at(0.72, 1.0)], 0.12),
        ] {
            painter.add(Shape::convex_polygon(pts, p.accent.gamma_multiply(a), Stroke::NONE));
        }
        painter.text(
            at(0.12, 0.05),
            Align2::LEFT_TOP,
            "Better stats.\nBigger goals.",
            FontId::proportional(12.0),
            p.weak,
        );
    }
    let chip = Rect::from_min_size(pos2(ui.max_rect().left(), bottom - 48.0), vec2(width, 48.0));
    let resp = ui.interact(chip, egui::Id::new("profile-chip"), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(chip, CornerRadius::same(10), p.selected);
    }
    let (name, status, color) = profile_status(state, &p);
    let avatar = Rect::from_center_size(chip.left_center() + vec2(22.0, 0.0), vec2(34.0, 34.0));
    let icon_uri = state.career.as_ref().and_then(|c| c.profile.card_icon.clone());
    if icon_uri.is_some() {
        theme::image_cover(ui, avatar, icon_uri.as_deref(), 1.0, 17);
    } else {
        ui.painter()
            .circle_filled(avatar.center(), 17.0, p.accent.gamma_multiply(0.18));
        let initials: String = name.chars().take(1).collect();
        ui.painter().text(
            avatar.center(),
            Align2::CENTER_CENTER,
            initials.to_uppercase(),
            FontId::proportional(14.0),
            p.accent,
        );
    }
    ui.painter().text(
        chip.left_center() + vec2(46.0, -8.0),
        Align2::LEFT_CENTER,
        &name,
        FontId::proportional(13.0),
        p.text,
    );
    ui.painter()
        .circle_filled(chip.left_center() + vec2(50.0, 9.0), 3.5, color);
    ui.painter().text(
        chip.left_center() + vec2(58.0, 9.0),
        Align2::LEFT_CENTER,
        status,
        FontId::proportional(11.0),
        p.weak,
    );
    if resp
        .on_hover_text("Connection details")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        state.screen = Screen::Connection;
    }
}

/// (name, status text, status colour) for the profile chip.
fn profile_status(state: &AppState, p: &theme::Palette) -> (String, String, Color32) {
    let name = state
        .career
        .as_ref()
        .and_then(|c| c.riot_id.as_ref())
        .map(|i| i.game_name.clone())
        .unwrap_or_else(|| "Not connected".into());
    let (status, color) = match &state.status {
        ClientStatus::Authed { .. } => match &state.game {
            Some(g) if g.in_queue() => ("In queue".to_string(), p.warn),
            Some(g) if g.loop_state.is_live() => ("In a match".to_string(), p.warn),
            _ => ("Online".to_string(), p.win),
        },
        ClientStatus::NotRunning => ("VALORANT isn't running".into(), p.loss),
        ClientStatus::SignedOut => ("Waiting for sign-in…".into(), p.warn),
        ClientStatus::Lost => ("Reconnecting…".into(), p.loss),
        ClientStatus::Connecting(_) => ("Connecting…".into(), p.warn),
    };
    (name, status, color)
}

fn top_bar(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    let p = pal(ui.ctx());
    ui.horizontal(|ui| {
        // Search: "Name#TAG" looks a player up; anything else filters the
        // Matches / Agents pages.
        let w = (ui.available_width() * 0.5).clamp(260.0, 560.0);
        let (rect, _) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
        ui.painter().rect(
            rect,
            CornerRadius::same(19),
            p.card,
            Stroke::new(1.0_f32, p.stroke),
            egui::StrokeKind::Inside,
        );
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.left_center() + vec2(20.0, 0.0), vec2(15.0, 15.0)),
            p.weak,
            Icon::Search,
        );
        let edit_rect = Rect::from_min_max(rect.min + vec2(38.0, 9.0), rect.max - vec2(14.0, 7.0));
        let resp = ui.put(
            edit_rect,
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Search for a player (Name#TAG), agent or map…")
                .frame(false)
                .font(FontId::proportional(13.0)),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            run_search(state, intents);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (r, toggle) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::click());
            ui.painter().rect(
                r,
                CornerRadius::same(19),
                p.card,
                Stroke::new(1.0_f32, p.stroke),
                egui::StrokeKind::Inside,
            );
            let icon = if p.dark { Icon::Moon } else { Icon::Sun };
            icons::paint(ui.painter(), r.shrink(11.0), p.text, icon);
            if toggle.on_hover_text("Switch light / dark").clicked() {
                state.settings.theme = if p.dark { Theme::Light } else { Theme::Dark };
                state.save_settings(intents);
            }
            ui.add_space(6.0);
            let title = match state.screen {
                Screen::Home => "",
                Screen::Live => "Live",
                Screen::Matches => "Matches",
                Screen::Agents => "Agents",
                Screen::Lookup => "Players",
                Screen::Crosshairs => "Crosshairs",
                Screen::Social => "Social",
                Screen::Settings => "Settings",
                Screen::Connection => "Connection",
                Screen::MatchDetail => "Match",
            };
            ui.label(RichText::new(title).size(12.0).color(p.faint));
        });
    });
}

fn run_search(state: &mut AppState, intents: &IntentTx) {
    let q = state.search.trim().to_string();
    if q.is_empty() {
        return;
    }
    if let Some((name, tag)) = lookup::parse_riot_id(&q) {
        state.lookup.input = q.clone();
        state.lookup.loading = true;
        state.lookup.result = None;
        state.search.clear();
        state.screen = Screen::Lookup;
        intents.send(Intent::LookupPlayer { name, tag });
    } else if state.catalog.agents.values().any(|a| a.name.eq_ignore_ascii_case(&q)) {
        state.screen = Screen::Agents;
    } else {
        state.screen = Screen::Matches;
    }
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
    let p = pal(ctx);
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(Align2::RIGHT_BOTTOM, vec2(-20.0, -20.0))
        .show(ctx, |ui| {
            for t in &state.toasts {
                theme::card_frame(&p)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.horizontal(|ui| {
                            ui.painter()
                                .circle_filled(ui.cursor().min + vec2(4.0, 9.0), 4.0, p.accent);
                            ui.add_space(14.0);
                            ui.label(RichText::new(&t.text).size(12.5).color(p.text));
                        });
                    });
                ui.add_space(8.0);
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
