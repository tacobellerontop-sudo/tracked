//! Built-in social: Riot friends with live status, and whispers, through the
//! Riot Client's own chat. Works without any backend.

use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, Layout, Rect, RichText, Sense, Stroke, Ui};

use super::icons::{self, Icon};
use super::theme::{self, pal, Palette};
use crate::app::AppState;
use crate::content::valorant_api::queue_name;
use crate::ipc::{ClientStatus, Intent, IntentTx};
use crate::riot::chat::{Activity, Friend, FriendStatus, MAX_MESSAGE};

const LIST_W: f32 = 320.0;

pub fn ui(ui: &mut Ui, state: &mut AppState, intents: &IntentTx) {
    let p = pal(ui.ctx());
    if !matches!(state.status, ClientStatus::Authed { .. }) {
        empty(
            ui,
            &p,
            Icon::Users,
            "Start VALORANT to see your friends",
            "Your Riot friends, what they're playing, and whispers show up here.",
        );
        return;
    }
    if !state.riot_social.requested {
        state.riot_social.requested = true;
        intents.send(Intent::RiotSocialRefresh);
    }
    let height = (ui.available_height() - 4.0).max(480.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.allocate_ui_with_layout(vec2(LIST_W, height), Layout::top_down(Align::Min), |ui| {
            ui.set_width(LIST_W);
            ui.set_min_height(height);
            ui.spacing_mut().item_spacing.x = 6.0;
            friend_list(ui, state, intents, &p);
        });
        // Divider.
        let (div, _) = ui.allocate_exact_size(vec2(1.0, height), Sense::hover());
        ui.painter().rect_filled(div, 0.0, p.stroke);
        ui.allocate_ui_with_layout(vec2(ui.available_width(), height), Layout::top_down(Align::Min), |ui| {
            ui.set_min_height(height);
            ui.spacing_mut().item_spacing.x = 6.0;
            conversation(ui, state, intents, &p);
        });
    });
}

fn empty(ui: &mut Ui, p: &Palette, icon: Icon, title: &str, body: &str) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
        ui.painter()
            .circle_filled(r.center(), 32.0, p.accent.gamma_multiply(0.12));
        icons::paint(ui.painter(), r.shrink(18.0), p.accent, icon);
        ui.add_space(10.0);
        ui.label(RichText::new(title).font(theme::semibold(16.0)).color(p.text));
        ui.label(RichText::new(body).size(12.5).color(p.weak));
    });
}

fn status_of<'a>(state: &'a AppState, f: &Friend) -> Option<&'a FriendStatus> {
    state.riot_social.statuses.get(&f.puuid)
}

fn activity_color(a: &Activity, p: &Palette) -> Color32 {
    match a {
        Activity::Offline => p.faint,
        Activity::Away => p.warn,
        Activity::InQueue | Activity::AgentSelect | Activity::InGame => p.accent,
        _ => p.win,
    }
}

/// "In game · Competitive · Ascent · 8–5", "In queue · Unrated", "Offline".
fn status_line(state: &AppState, s: Option<&FriendStatus>) -> String {
    let Some(s) = s else { return "Offline".into() };
    let mut parts = vec![s.activity.label()];
    if matches!(
        s.activity,
        Activity::InGame | Activity::AgentSelect | Activity::InQueue | Activity::Menus
    ) {
        if !s.queue_id.is_empty() {
            parts.push(queue_name(&s.queue_id).to_string());
        }
        if !s.map.is_empty() && s.activity != Activity::Menus {
            parts.push(state.catalog.map_name(&s.map));
        }
        if let Some((a, b)) = s.score {
            parts.push(format!("{a}–{b}"));
        }
        if s.party_size > 1 && s.activity != Activity::InGame {
            parts.push(format!("party of {}", s.party_size));
        }
    }
    parts.join(" · ")
}

fn friend_list(ui: &mut Ui, state: &mut AppState, intents: &IntentTx, p: &Palette) {
    let rs = &state.riot_social;
    let online = rs
        .friends
        .iter()
        .filter(|f| status_of(state, f).is_some_and(|s| s.activity.online()))
        .count();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Friends").font(theme::semibold(16.0)).color(p.text));
        ui.label(RichText::new(format!("{online} online")).size(11.5).color(p.weak));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(14.0);
            let r = ui.add(
                egui::Button::new(
                    RichText::new(egui_phosphor::regular::ARROWS_CLOCKWISE)
                        .font(theme::icon_font(15.0))
                        .color(p.weak),
                )
                .frame(false),
            );
            if r.on_hover_text("Refresh").clicked() {
                intents.send(Intent::RiotSocialRefresh);
            }
        });
    });
    ui.add_space(8.0);

    // Search.
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(LIST_W - 16.0, 34.0), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(17), p.well);
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(14.0, 14.0)),
            p.weak,
            Icon::Search,
        );
        let edit = Rect::from_min_max(rect.min + vec2(34.0, 8.0), rect.max - vec2(12.0, 6.0));
        ui.put(
            edit,
            egui::TextEdit::singleline(&mut state.riot_social.filter)
                .hint_text("Search friends")
                .frame(false),
        );
    });
    ui.add_space(8.0);

    // Friend requests (read-only; accept them in the Riot Client).
    let incoming: Vec<_> = state
        .riot_social
        .requests
        .iter()
        .filter(|r| r.incoming())
        .cloned()
        .collect();
    if !incoming.is_empty() {
        let label = format!(
            "{} friend request{}",
            incoming.len(),
            if incoming.len() == 1 { "" } else { "s" }
        );
        let r = ui
            .horizontal(|ui| {
                icons::icon(ui, Icon::UserPlus, 14.0, p.accent);
                ui.add(egui::Button::new(RichText::new(label).size(12.5).color(p.accent)).frame(false))
            })
            .inner;
        if r.clicked() {
            state.riot_social.show_requests = !state.riot_social.show_requests;
        }
        if state.riot_social.show_requests {
            for req in &incoming {
                ui.label(
                    RichText::new(format!("   {}#{}", req.game_name, req.game_tag))
                        .size(12.5)
                        .color(p.text),
                );
            }
            ui.label(
                RichText::new("   Accept or decline in the Riot Client.")
                    .size(11.0)
                    .color(p.faint),
            );
        }
        ui.add_space(4.0);
    }

    if !state.riot_social.loaded {
        for _ in 0..6 {
            let (r, _) = ui.allocate_exact_size(vec2(LIST_W - 16.0, 48.0), Sense::hover());
            theme::skeleton(ui, r.shrink2(vec2(0.0, 4.0)), 12);
        }
        return;
    }
    let q = state.riot_social.filter.trim().to_lowercase();
    let mut friends: Vec<Friend> = state
        .riot_social
        .friends
        .iter()
        .filter(|f| q.is_empty() || f.riot_id().to_lowercase().contains(&q) || f.note.to_lowercase().contains(&q))
        .cloned()
        .collect();
    let rank = |s: Option<&FriendStatus>| match s.map(|s| &s.activity) {
        Some(a) if a.busy() => 0,
        Some(a) if a.online() => 1,
        _ => 2,
    };
    friends.sort_by_key(|f| (rank(status_of(state, f)), f.game_name.to_lowercase()));

    let mut open = None;
    egui::ScrollArea::vertical()
        .id_salt("riot-friends")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(LIST_W - 8.0);
            let mut section = None;
            for f in &friends {
                let s = status_of(state, f).cloned();
                let sec = rank(s.as_ref());
                if section != Some(sec) {
                    section = Some(sec);
                    let title = ["In a match", "Online", "Offline"][sec];
                    let count = friends.iter().filter(|x| rank(status_of(state, x)) == sec).count();
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(title.to_uppercase())
                                .font(theme::semibold(10.5))
                                .color(p.faint),
                        );
                        ui.label(RichText::new(count.to_string()).size(10.5).color(p.faint));
                        if sec == 2 {
                            let label = if state.riot_social.show_offline { "hide" } else { "show" };
                            if ui
                                .add(egui::Button::new(RichText::new(label).size(10.5).color(p.accent)).frame(false))
                                .clicked()
                            {
                                state.riot_social.show_offline = !state.riot_social.show_offline;
                            }
                        }
                    });
                    ui.add_space(2.0);
                }
                if sec == 2 && !state.riot_social.show_offline {
                    continue;
                }
                if friend_row(ui, state, f, s.as_ref(), p).clicked() {
                    open = Some(f.pid.clone());
                }
            }
            if friends.is_empty() {
                ui.add_space(20.0);
                ui.label(
                    RichText::new(if q.is_empty() {
                        "No friends yet."
                    } else {
                        "No friends match."
                    })
                    .color(p.weak),
                );
            }
        });
    if let Some(cid) = open {
        open_chat(state, intents, &cid);
    }
}

pub fn open_chat(state: &mut AppState, intents: &IntentTx, cid: &str) {
    state.riot_social.open = Some(cid.to_string());
    state.riot_social.unread.remove(cid);
    intents.send(Intent::RiotChatOpen(cid.to_string()));
}

fn friend_row(ui: &mut Ui, state: &AppState, f: &Friend, s: Option<&FriendStatus>, p: &Palette) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 8.0, 52.0), Sense::click());
    let selected = state.riot_social.open.as_deref() == Some(f.pid.as_str());
    let hover = theme::glide(
        ui.ctx(),
        resp.id.with("h"),
        if resp.hovered() { 1.0 } else { 0.0 },
        0.12,
    );
    if selected || hover > 0.0 {
        let fill = if selected {
            p.selected
        } else {
            p.selected.gamma_multiply(0.6 * hover)
        };
        ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
    }
    let activity = s.map(|s| s.activity.clone()).unwrap_or_default();
    let dim = !activity.online();
    // Avatar with a status dot.
    let av = Rect::from_center_size(rect.left_center() + vec2(26.0, 0.0), vec2(36.0, 36.0));
    let card = s.and_then(|s| s.card_icon());
    if card.is_some() {
        theme::image_cover(ui, av, card.as_deref(), 1.0, 18);
    } else {
        ui.painter()
            .circle_filled(av.center(), 18.0, p.accent.gamma_multiply(0.15));
        let initial: String = f.game_name.chars().take(1).collect::<String>().to_uppercase();
        ui.painter().text(
            av.center(),
            Align2::CENTER_CENTER,
            initial,
            theme::semibold(14.0),
            p.accent,
        );
    }
    if dim {
        ui.painter()
            .circle_filled(av.center(), 18.0, p.card.gamma_multiply(0.45));
    }
    let dot = av.right_bottom() - vec2(4.0, 4.0);
    ui.painter()
        .circle_filled(dot, 6.0, if selected { p.selected } else { p.card });
    ui.painter().circle_filled(dot, 4.0, activity_color(&activity, p));

    let x = av.right() + 12.0;
    let name_color = if dim { p.weak } else { p.text };
    let name = ui.painter().text(
        pos2(x, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        &f.game_name,
        theme::medium(13.0),
        name_color,
    );
    ui.painter().text(
        pos2(name.right() + 4.0, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        format!("#{}", f.game_tag),
        egui::FontId::proportional(11.0),
        p.faint,
    );
    let line = status_line(state, s);
    let max_chars = 34;
    let line = if line.chars().count() > max_chars {
        format!("{}…", line.chars().take(max_chars).collect::<String>())
    } else {
        line
    };
    ui.painter().text(
        pos2(x, rect.center().y + 9.0),
        Align2::LEFT_CENTER,
        line,
        egui::FontId::proportional(11.0),
        if activity.busy() { p.accent } else { p.weak },
    );

    // Right side: unread badge, else rank icon.
    let unread = state.riot_social.unread.get(&f.pid).copied().unwrap_or(0);
    if unread > 0 {
        let c = rect.right_center() - vec2(16.0, 0.0);
        ui.painter().circle_filled(c, 9.0, p.accent);
        ui.painter().text(
            c,
            Align2::CENTER_CENTER,
            unread.min(99).to_string(),
            theme::semibold(10.0),
            Color32::WHITE,
        );
    } else if let Some(tier) = s.map(|s| s.competitive_tier).filter(|t| *t > 0) {
        if let Some(icon) = state
            .catalog
            .tier(tier)
            .map(|t| t.icon.clone())
            .filter(|u| !u.is_empty())
        {
            let r = Rect::from_center_size(rect.right_center() - vec2(18.0, 0.0), vec2(22.0, 22.0));
            egui::Image::new(icon).paint_at(ui, r);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn conversation(ui: &mut Ui, state: &mut AppState, intents: &IntentTx, p: &Palette) {
    let Some(cid) = state.riot_social.open.clone() else {
        empty(
            ui,
            p,
            Icon::Chat,
            "Pick a friend to whisper",
            "Messages go through your Riot Client, the same as in-game whispers.",
        );
        return;
    };
    let friend = state.riot_social.friend_by_cid(&cid).cloned();
    let status = friend
        .as_ref()
        .and_then(|f| state.riot_social.statuses.get(&f.puuid))
        .cloned();
    let me = state.self_puuid().unwrap_or_default().to_string();

    // Header.
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        let name = friend
            .as_ref()
            .map(|f| f.game_name.clone())
            .unwrap_or_else(|| "Conversation".into());
        let card = status.as_ref().and_then(|s| s.card_icon());
        let initial: String = name.chars().take(1).collect::<String>().to_uppercase();
        theme::avatar(ui, card.as_deref(), &initial, 38.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(&name).font(theme::semibold(15.0)).color(p.text));
                if let Some(f) = &friend {
                    ui.label(RichText::new(format!("#{}", f.game_tag)).size(11.5).color(p.faint));
                }
            });
            let activity = status.as_ref().map(|s| s.activity.clone()).unwrap_or_default();
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(8.0, 12.0), Sense::hover());
                ui.painter()
                    .circle_filled(r.center(), 3.5, activity_color(&activity, p));
                ui.label(
                    RichText::new(status_line(state, status.as_ref()))
                        .size(11.5)
                        .color(p.weak),
                );
            });
        });
    });
    ui.add_space(8.0);
    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(line, 0.0, p.stroke);

    // Messages.
    let input_h = 64.0;
    let msgs = state.riot_social.messages.get(&cid).cloned().unwrap_or_default();
    egui::ScrollArea::vertical()
        .id_salt(("whispers", &cid))
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .max_height((ui.available_height() - input_h).max(120.0))
        .show(ui, |ui| {
            ui.add_space(12.0);
            if msgs.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new("No messages yet. Say hi!").color(p.weak));
                });
            }
            let mut last_day = String::new();
            for m in &msgs {
                let t = chrono::DateTime::from_timestamp_millis(m.time_ms()).map(|d| d.with_timezone(&chrono::Local));
                let day = t.map(|d| d.format("%A, %b %d").to_string()).unwrap_or_default();
                if day != last_day {
                    last_day = day.clone();
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(day).size(10.5).color(p.faint));
                    });
                    ui.add_space(4.0);
                }
                bubble(
                    ui,
                    p,
                    &m.body,
                    m.puuid == me,
                    t.map(|d| d.format("%H:%M").to_string()).unwrap_or_default(),
                );
            }
            ui.add_space(8.0);
        });

    // Composer.
    let sending = state.riot_social.sending.get(&cid).copied().unwrap_or(false);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        let w = ui.available_width() - 16.0;
        let (rect, _) = ui.allocate_exact_size(vec2(w, 44.0), Sense::hover());
        ui.painter().rect(
            rect,
            CornerRadius::same(22),
            p.well,
            Stroke::new(1.0_f32, p.stroke),
            egui::StrokeKind::Inside,
        );
        let draft = state.riot_social.drafts.entry(cid.clone()).or_default();
        let edit_rect = Rect::from_min_max(rect.min + vec2(18.0, 12.0), rect.max - vec2(56.0, 10.0));
        let name = friend.as_ref().map(|f| f.game_name.clone()).unwrap_or_default();
        let resp = ui.put(
            edit_rect,
            egui::TextEdit::singleline(draft)
                .hint_text(format!("Message {name}"))
                .frame(false)
                .char_limit(MAX_MESSAGE),
        );
        let send_rect = Rect::from_center_size(rect.right_center() - vec2(24.0, 0.0), vec2(34.0, 34.0));
        let can_send = !draft.trim().is_empty() && !sending;
        let send = ui.interact(send_rect, ui.id().with(("send", &cid)), Sense::click());
        ui.painter()
            .circle_filled(send_rect.center(), 17.0, if can_send { p.accent } else { p.stroke });
        icons::paint(ui.painter(), send_rect.shrink(9.0), Color32::WHITE, Icon::Send);
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (send.clicked() || enter) && can_send {
            let text = std::mem::take(draft);
            state.riot_social.sending.insert(cid.clone(), true);
            intents.send(Intent::RiotChatSend { cid: cid.clone(), text });
            resp.request_focus();
        }
    });
}

fn bubble(ui: &mut Ui, p: &Palette, text: &str, mine: bool, time: String) {
    let max_w = (ui.available_width() * 0.62).max(160.0);
    let layout = if mine {
        Layout::right_to_left(Align::TOP)
    } else {
        Layout::left_to_right(Align::TOP)
    };
    ui.with_layout(layout, |ui| {
        ui.add_space(20.0);
        let (fill, color) = if mine {
            (p.accent, Color32::WHITE)
        } else {
            (p.well, p.text)
        };
        let galley = ui
            .painter()
            .layout(text.to_string(), egui::FontId::proportional(13.0), color, max_w - 28.0);
        let size = galley.size() + vec2(28.0, 18.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
        let r = if mine {
            CornerRadius {
                nw: 16,
                ne: 16,
                sw: 16,
                se: 4,
            }
        } else {
            CornerRadius {
                nw: 16,
                ne: 16,
                sw: 4,
                se: 16,
            }
        };
        ui.painter().rect_filled(rect, r, fill);
        ui.painter().galley(rect.min + vec2(14.0, 9.0), galley, color);
        resp.on_hover_text(time);
    });
    ui.add_space(4.0);
}
