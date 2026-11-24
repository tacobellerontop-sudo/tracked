//! Top-level layout: sidebar, top bar (search + theme), content, toasts.

pub mod career;
pub mod connection;
pub mod crosshairs;
pub mod friends;
pub mod home;
pub mod icons;
pub mod live;
pub mod loadout;
pub mod lookup;
pub mod mastery;
pub mod match_detail;
pub mod pages;
pub mod profile;
pub mod recaps;
pub mod replay;
pub mod settings;
pub mod social;
pub mod store;
pub mod theme;
pub mod titlebar;
pub mod weapons;
pub mod widgets;

use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Shape, Stroke};

use crate::app::{AppState, Screen, SocialTab, Toast};
use crate::backend::models::MinVersionState;
use crate::config::Theme;
use crate::ipc::{BackendStatus, ClientStatus, Intent, IntentTx};
use icons::Icon;
use theme::pal;

pub const DISCLAIMER: &str = "Tracked is an unofficial third-party tool. It is not affiliated with, \
endorsed, sponsored or approved by Riot Games. VALORANT and Riot Games are trademarks of Riot Games, Inc. \
You use this app at your own risk and remain subject to Riot's Terms of Service.\n\n\
Tracked only uses the Riot Client's local API, Riot's web endpoints with tokens the client issued, \
and replay files VALORANT has downloaded to your PC (read from disk). \
It never reads game memory, never injects into or modifies any Riot process, and never automates gameplay.";

/// Sidebar items in sections (`None` = no heading); the key (if any) is
/// the Ctrl+<key> shortcut.
type NavItem = (Screen, &'static str, Icon, Option<egui::Key>);
const NAV_SECTIONS: &[(Option<&str>, &[NavItem])] = &[
    (
        None,
        &[
            (Screen::Home, "Home", Icon::Home, Some(egui::Key::Num1)),
            (Screen::Live, "Live", Icon::Live, Some(egui::Key::Num2)),
        ],
    ),
    (
        Some("Stats"),
        &[
            (Screen::Matches, "Matches", Icon::Matches, Some(egui::Key::Num3)),
            (Screen::Recaps, "Recaps", Icon::Recap, Some(egui::Key::Num4)),
            (Screen::Mastery, "Mastery", Icon::Trophy, Some(egui::Key::Num5)),
            (Screen::Agents, "Agents", Icon::Agents, Some(egui::Key::Num6)),
            (Screen::Weapons, "Weapons", Icon::Target, Some(egui::Key::Num7)),
            (Screen::Replays, "Replays", Icon::Map, None),
        ],
    ),
    (
        Some("Account"),
        &[
            (Screen::Store, "Store", Icon::Store, None),
            (Screen::Loadout, "Loadout", Icon::Loadout, None),
        ],
    ),
    (
        Some("Community"),
        &[
            (Screen::Lookup, "Players", Icon::Search, Some(egui::Key::Num8)),
            (Screen::Social, "Social", Icon::Social, Some(egui::Key::Num9)),
        ],
    ),
    (
        Some("Tools"),
        &[
            (Screen::Crosshairs, "Crosshairs", Icon::Crosshair, None),
            (Screen::Settings, "Settings", Icon::Settings, Some(egui::Key::Num0)),
        ],
    ),
];

fn nav_items() -> impl Iterator<Item = &'static NavItem> {
    NAV_SECTIONS.iter().flat_map(|(_, items)| items.iter())
}

/// Title and subtitle shown at the top of each screen.
fn page_heading(state: &AppState) -> (&'static str, String) {
    let n = state.career.as_ref().map(|c| c.matches.len()).unwrap_or(0);
    match state.screen {
        Screen::Home => {
            let name = state
                .career
                .as_ref()
                .and_then(|c| c.riot_id.as_ref())
                .map(|i| i.game_name.clone());
            (
                "Overview",
                name.map(|n| format!("Welcome back, {n}")).unwrap_or_default(),
            )
        }
        Screen::Live => ("Live", "Your lobby, ranks and recent form".into()),
        Screen::Matches => ("Matches", format!("Your last {n} games")),
        Screen::Recaps => ("Recaps", "Your play sessions and overall stats".into()),
        Screen::Mastery => ("Mastery", "Agent mastery and the accolades you've earned".into()),
        Screen::Agents => ("Agents", "How you play each agent".into()),
        Screen::Weapons => ("Weapons", "Kills, damage and accuracy per gun".into()),
        Screen::Replays => ("Replays", "Watch any match top-down".into()),
        Screen::Store => ("Store", "Today's offers, bundles and your wallet".into()),
        Screen::Loadout => ("Loadout", "What you have equipped and own".into()),
        Screen::Lookup => ("Players", "Look anyone up by Riot ID".into()),
        Screen::Crosshairs => ("Crosshairs", "Build, preview and save crosshair codes".into()),
        Screen::Social => ("Social", "Friends and whispers".into()),
        Screen::Settings => ("Settings", String::new()),
        Screen::Connection => ("Connection", "How Tracked talks to the Riot Client".into()),
        Screen::MatchDetail => ("Match", String::new()),
        Screen::Replay => ("Replay", String::new()),
    }
}

const SIDEBAR_W: f32 = 210.0;

pub fn draw(ctx: &egui::Context, state: &mut AppState, intents: &IntentTx) {
    let full_layout = state.settings.disclaimer_accepted
        && !matches!(state.social.min_version, MinVersionState::Blocked { .. })
        && state.startup_done;
    titlebar::show(ctx, state, if full_layout { SIDEBAR_W } else { 0.0 });
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
    // Social is always available: built-in friends come from the Riot Client itself.
    let visible = |_: &Screen| true;

    // Ctrl+<n> switches screens.
    for (screen, _, _, key) in nav_items().filter(|n| visible(&n.0)) {
        let Some(key) = key else { continue };
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
            top: if titlebar::enabled(state) { 8 } else { 22 },
            bottom: 16,
        }))
        .show(ctx, |ui| sidebar(ui, state, &visible));
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(p.bg).inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: if titlebar::enabled(state) { 4 } else { 18 },
            bottom: 0,
        }))
        .show(ctx, |ui| {
            ambient(ui);
            top_bar(ui, state, intents);
            ui.add_space(18.0);
            // Social manages its own scroll areas (friends, conversations, messages).
            if state.screen == Screen::Social {
                social_screen(ui, state, intents);
                return;
            }
            // The replay sizes its map to the window, so it has no outer scroll.
            if state.screen == Screen::Replay {
                theme::card(ui, None, |ui| replay::ui(ui, state, intents));
                return;
            }
            // Page fade-in when the screen changes.
            let now = ui.input(|i| i.time);
            if state.last_screen != Some(state.screen) {
                state.last_screen = Some(state.screen);
                state.screen_since = now;
            }
            let fade = if theme::animations(ui.ctx()) {
                theme::ease(((now - state.screen_since) / 0.25) as f32)
            } else {
                1.0
            };
            if fade < 1.0 {
                ui.ctx().request_repaint();
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.multiply_opacity(fade);
                ui.add_space((1.0 - fade) * 10.0);
                ui.spacing_mut().item_spacing.y = 16.0;
                match state.screen {
                    Screen::Home => home::ui(ui, state, intents),
                    Screen::Matches => pages::matches(ui, state, intents),
                    Screen::Replays => pages::replays(ui, state, intents),
                    Screen::Agents => pages::agents(ui, state),
                    Screen::Weapons => weapons::ui(ui, state),
                    Screen::Recaps => recaps::ui(ui, state, intents),
                    Screen::Mastery => mastery::ui(ui, state, intents),
                    Screen::Store => store::ui(ui, state, intents),
                    Screen::Loadout => loadout::ui(ui, state, intents),
                    Screen::Live => theme::card(ui, None, |ui| live::ui(ui, state, intents)),
                    Screen::Lookup => lookup::ui(ui, state, intents),
                    Screen::Crosshairs => theme::card(ui, None, |ui| crosshairs::ui(ui, state, intents)),
                    Screen::MatchDetail => theme::card(ui, None, |ui| match_detail::ui(ui, state, intents)),
                    Screen::Settings => theme::card(ui, None, |ui| settings::ui(ui, state, intents)),
                    Screen::Connection => theme::card(ui, None, |ui| connection::ui(ui, state, intents)),
                    Screen::Social | Screen::Replay => {}
                }
                ui.add_space(24.0);
            });
        });
}

fn sidebar(ui: &mut egui::Ui, state: &mut AppState, visible: &dyn Fn(&Screen) -> bool) {
    let p = pal(ui.ctx());
    if titlebar::enabled(state) {
        // The app mark and name live in the title bar.
        ui.add_space(6.0);
    } else {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
            icons::logo(ui.painter(), r, p.accent);
            ui.add_space(4.0);
            ui.label(RichText::new("T R A C K E D").font(theme::semibold(13.0)).color(p.text));
        });
        ui.add_space(22.0);
    }

    // The selected-item highlight is painted first (into a reserved slot)
    // and glides to the selected item.
    let highlight = ui.painter().add(Shape::Noop);
    let mut selected_rect = None;
    let unread = state.riot_social.unread_total();
    let item_h = 36.0;
    for (heading, items) in NAV_SECTIONS {
        if let Some(h) = heading {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(h.to_uppercase())
                        .font(theme::semibold(9.5))
                        .extra_letter_spacing(1.2)
                        .color(p.faint),
                );
            });
            ui.add_space(2.0);
        }
        for (screen, label, icon, _) in items.iter().filter(|n| visible(&n.0)) {
            let selected = nav_owner(state) == *screen;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), item_h), Sense::click());
            if selected {
                selected_rect = Some(rect);
            }
            let target = if resp.hovered() && !selected { 1.0 } else { 0.0 };
            let hover = theme::glide(ui.ctx(), resp.id.with("hover"), target, 0.12);
            if hover > 0.0 {
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(11), p.selected.gamma_multiply(0.6 * hover));
            }
            let color = if selected {
                p.text
            } else {
                p.weak.lerp_to_gamma(p.text, hover * 0.6)
            };
            let ic = Rect::from_center_size(rect.left_center() + vec2(22.0, 0.0), vec2(17.0, 17.0));
            icons::paint(ui.painter(), ic, if selected { p.accent } else { color }, *icon);
            let font = if selected {
                theme::semibold(13.0)
            } else {
                theme::medium(13.0)
            };
            ui.painter().text(
                rect.left_center() + vec2(42.0, 0.0),
                Align2::LEFT_CENTER,
                *label,
                font,
                color,
            );
            if *screen == Screen::Social && unread > 0 {
                let c = rect.right_center() - vec2(16.0, 0.0);
                ui.painter().circle_filled(c, 9.0, p.accent);
                ui.painter().text(
                    c,
                    Align2::CENTER_CENTER,
                    unread.min(99).to_string(),
                    theme::semibold(10.0),
                    Color32::WHITE,
                );
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                state.screen = *screen;
            }
            ui.add_space(1.0);
        }
    }
    if let Some(r) = selected_rect {
        let y = theme::glide(ui.ctx(), egui::Id::new("nav-highlight"), r.top(), 0.22);
        let rect = Rect::from_min_size(pos2(r.left(), y), r.size());
        let bar = Rect::from_min_size(rect.left_top() + vec2(0.0, 9.0), vec2(3.0, rect.height() - 18.0));
        // A red-to-clear wash with a glowing edge bar.
        let mut shapes = vec![Shape::rect_filled(rect, CornerRadius::same(11), p.selected)];
        let mut mesh = egui::Mesh::default();
        let wash = rect.shrink2(vec2(0.0, 0.0));
        let a = p.accent.gamma_multiply(if p.dark { 0.20 } else { 0.10 });
        mesh.colored_vertex(wash.left_top(), a);
        mesh.colored_vertex(wash.right_top(), Color32::TRANSPARENT);
        mesh.colored_vertex(wash.left_bottom(), a);
        mesh.colored_vertex(wash.right_bottom(), Color32::TRANSPARENT);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 2, 3);
        shapes.push(Shape::mesh(mesh));
        shapes.push(Shape::rect_filled(
            bar.expand2(vec2(3.0, 2.0)),
            CornerRadius::same(4),
            p.accent.gamma_multiply(0.3),
        ));
        shapes.push(Shape::rect_filled(bar, CornerRadius::same(2), p.accent));
        ui.painter().set(highlight, Shape::Vec(shapes));
    }

    // Bottom: profile chip on a subtle panel.
    let bottom = ui.max_rect().bottom();
    let width = ui.available_width();
    let chip = Rect::from_min_size(pos2(ui.max_rect().left(), bottom - 54.0), vec2(width, 54.0));
    if chip.top() < ui.cursor().top() + 4.0 {
        return;
    }
    let resp = ui.interact(chip, egui::Id::new("profile-chip"), Sense::click());
    ui.painter().rect(
        chip,
        CornerRadius::same(14),
        if resp.hovered() { p.selected } else { p.card },
        Stroke::new(1.0_f32, p.stroke),
        egui::StrokeKind::Inside,
    );
    let (name, status, color) = profile_status(state, &p);
    let avatar = Rect::from_center_size(chip.left_center() + vec2(26.0, 0.0), vec2(34.0, 34.0));
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
            theme::semibold(14.0),
            p.accent,
        );
    }
    ui.painter()
        .circle_filled(avatar.right_bottom() - vec2(3.0, 3.0), 5.5, p.card);
    ui.painter()
        .circle_filled(avatar.right_bottom() - vec2(3.0, 3.0), 3.8, color);
    ui.painter().text(
        chip.left_center() + vec2(52.0, -8.0),
        Align2::LEFT_CENTER,
        &name,
        theme::semibold(12.5),
        p.text,
    );
    ui.painter().text(
        chip.left_center() + vec2(52.0, 9.0),
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

/// Soft red and violet glows behind the content area.
fn ambient(ui: &egui::Ui) {
    let p = pal(ui.ctx());
    let r = ui.max_rect();
    let painter = ui.painter().with_clip_rect(r.expand(24.0));
    let k = if p.dark { 1.0 } else { 0.45 };
    theme::radial_glow(
        &painter,
        pos2(r.right() - r.width() * 0.08, r.top() - 40.0),
        r.width().max(600.0) * 0.55,
        p.accent.gamma_multiply(0.10 * k),
    );
    theme::radial_glow(
        &painter,
        pos2(r.left() + r.width() * 0.12, r.bottom() + 60.0),
        r.width().max(600.0) * 0.5,
        p.accent2.gamma_multiply(0.09 * k),
    );
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
    let (title, subtitle) = page_heading(state);
    ui.horizontal(|ui| {
        ui.set_min_height(46.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(title).font(theme::semibold(24.0)).color(p.text));
            if !subtitle.is_empty() {
                ui.label(RichText::new(subtitle).size(12.5).color(p.weak));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (r, toggle) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::click());
            ui.painter().rect(
                r,
                CornerRadius::same(12),
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
            ui.add_space(4.0);
            // Search: "Name#TAG" looks a player up; anything else filters the
            // Matches / Agents pages.
            let w = (ui.available_width() * 0.55).clamp(220.0, 380.0);
            let (rect, _) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
            ui.painter().rect(
                rect,
                CornerRadius::same(12),
                p.card,
                Stroke::new(1.0_f32, p.stroke),
                egui::StrokeKind::Inside,
            );
            icons::paint(
                ui.painter(),
                Rect::from_center_size(rect.left_center() + vec2(19.0, 0.0), vec2(14.0, 14.0)),
                p.weak,
                Icon::Search,
            );
            let edit_rect = Rect::from_min_max(rect.min + vec2(36.0, 10.0), rect.max - vec2(12.0, 8.0));
            let resp = ui.put(
                edit_rect,
                egui::TextEdit::singleline(&mut state.search)
                    .hint_text("Search Name#TAG, agent or map…")
                    .frame(false)
                    .font(FontId::proportional(12.5)),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                run_search(state, intents);
            }
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
    } else if crate::content::weapons::is_weapon_name(&q)
        || state.catalog.weapons.values().any(|w| w.name.eq_ignore_ascii_case(&q))
    {
        state.screen = Screen::Weapons;
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
            "The configured backend requires Tracked v{required} or newer. You're running v{}.",
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

/// Social: built-in Riot friends and whispers, plus the optional Tracked backend tab.
fn social_screen(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    let backend_on = state.social.status != BackendStatus::Disabled;
    if !backend_on {
        state.riot_social.tab = SocialTab::Friends;
    }
    if backend_on {
        ui.horizontal(|ui| {
            for (tab, label) in [(SocialTab::Friends, "Riot friends"), (SocialTab::Tracked, "Tracked")] {
                if theme::pill(ui, label, state.riot_social.tab == tab).clicked() {
                    state.riot_social.tab = tab;
                }
            }
        });
        ui.add_space(10.0);
    }
    match state.riot_social.tab {
        SocialTab::Friends => theme::card(ui, None, |ui| friends::ui(ui, state, intents)),
        SocialTab::Tracked => theme::card(ui, None, |ui| social::ui(ui, state, intents)),
    }
}

/// The sidebar item a screen belongs to (match detail and replay highlight
/// the page they were opened from).
fn nav_owner(state: &AppState) -> Screen {
    let mut screen = state.screen;
    if screen == Screen::Replay {
        screen = state.replay.return_to.unwrap_or(Screen::Matches);
    }
    if screen == Screen::MatchDetail {
        screen = state.return_to;
    }
    screen
}
