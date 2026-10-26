//! Social (optional backend): friends with presence and lobbies, and DMs.

use std::time::{Duration, Instant};

use egui::{Color32, RichText};

use super::widgets;
use crate::app::{AppState, Screen};
use crate::ipc::{BackendStatus, Intent, IntentTx};

const TYPING_TTL: Duration = Duration::from_secs(4);
const TYPING_SEND_EVERY: Duration = Duration::from_secs(3);

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.horizontal(|ui| {
        ui.heading("Social");
        if state.social.me().is_some() {
            let (c, t) = if state.social.connected {
                (widgets::WIN, "live")
            } else {
                (Color32::GRAY, "reconnecting…")
            };
            ui.colored_label(c, t);
            if ui.button("Refresh").clicked() {
                intents.send(Intent::SocialRefresh);
            }
            if ui.button("Sign out").clicked() {
                intents.send(Intent::BackendSignOut);
            }
        }
    });
    if let Some(motd) = state.social.config.as_ref().and_then(|c| c.motd.clone()) {
        ui.weak(motd);
    }
    ui.add_space(6.0);

    match state.social.status.clone() {
        BackendStatus::Disabled => {
            ui.label("Social features are off. They need an Ascend-compatible backend URL (Settings → Social).");
            if ui.button("Open Settings").clicked() {
                state.screen = Screen::Settings;
            }
        }
        BackendStatus::Error(e) => {
            ui.colored_label(widgets::LOSS, e);
            if ui.button("Retry").clicked() {
                intents.send(Intent::BackendConfigure);
            }
        }
        BackendStatus::SignedOut | BackendStatus::SigningIn => sign_in(ui, state, intents),
        BackendStatus::SignedIn(_) => signed_in(ui, state, intents),
    }
}

fn sign_in(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.label("Sign in to see friends' status and send messages.");
    ui.weak("Your session token stays in memory and is never saved to disk.");
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut state.social.sign_in_name).hint_text("Display name"));
        let busy = state.social.status == BackendStatus::SigningIn;
        let ok = !state.social.sign_in_name.trim().is_empty() && !busy;
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.add_enabled(ok, egui::Button::new("Sign in")).clicked() || (enter && ok)) && ok {
            intents.send(Intent::BackendSignIn {
                display_name: state.social.sign_in_name.trim().to_string(),
            });
        }
        if busy {
            ui.spinner();
        }
    });
}

fn signed_in(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    let height = ui.available_height().max(420.0);
    ui.horizontal_top(|ui| {
        ui.allocate_ui(egui::vec2(280.0, height), |ui| {
            ui.set_width(280.0);
            ui.vertical(|ui| friends(ui, state, intents));
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.set_min_height(height);
            conversation(ui, state, intents);
        });
    });
}

fn status_of(state: &AppState, id: &str) -> (Color32, String) {
    let Some(p) = state.social.presence.get(id).filter(|p| p.online) else {
        return (Color32::GRAY, "Offline".into());
    };
    let queue = p
        .queue
        .as_deref()
        .map(crate::content::valorant_api::queue_name)
        .unwrap_or_default()
        .to_string();
    match p.state.as_deref() {
        Some("INGAME") => (Color32::from_rgb(230, 160, 60), format!("In game · {queue}")),
        Some("PREGAME") => (Color32::from_rgb(230, 200, 80), format!("Agent select · {queue}")),
        Some("MENUS") => (widgets::WIN, "In menus".into()),
        _ => (widgets::WIN, "Online".into()),
    }
}

fn friends(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.label(RichText::new("Friends").strong());
    let mut friends = state.social.friends.clone();
    friends.sort_by_key(|f| {
        (
            !state.social.presence.get(&f.id).is_some_and(|p| p.online),
            f.display_name.to_lowercase(),
        )
    });
    let mut message = None;
    egui::ScrollArea::vertical()
        .id_salt("friends")
        .max_height(260.0)
        .show(ui, |ui| {
            for f in &friends {
                let (color, text) = status_of(state, &f.id);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.5, color);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&f.display_name).strong());
                        ui.weak(text);
                        if let Some(l) = state.social.lobbies.get(&f.id) {
                            let queue = crate::content::valorant_api::queue_name(&l.queue);
                            let open = if l.open { "open" } else { "full" };
                            ui.colored_label(
                                widgets::WIN,
                                format!("Lobby {}/{} · {queue} · {open}", l.size, l.max.max(l.size)),
                            );
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Message").clicked() {
                            message = Some(f.id.clone());
                        }
                        let mut picked = state.social.group_pick.contains(&f.id);
                        if ui
                            .checkbox(&mut picked, "")
                            .on_hover_text("Select for a group chat")
                            .changed()
                        {
                            if picked {
                                state.social.group_pick.insert(f.id.clone());
                            } else {
                                state.social.group_pick.remove(&f.id);
                            }
                        }
                    });
                });
                ui.add_space(4.0);
            }
        });
    if let Some(friend) = message {
        open_direct(state, intents, &friend);
    }

    if !state.social.group_pick.is_empty() {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.social.group_name)
                    .hint_text("Group name")
                    .desired_width(150.0),
            );
            if ui
                .button(format!("Create group ({})", state.social.group_pick.len()))
                .clicked()
            {
                let members: Vec<String> = state.social.group_pick.drain().collect();
                let name = state.social.group_name.trim().to_string();
                state.social.open_next = true;
                intents.send(Intent::DmCreate {
                    members,
                    name: (!name.is_empty()).then_some(name),
                });
                state.social.group_name.clear();
            }
        });
    }

    ui.separator();
    ui.label(RichText::new("Messages").strong());
    let mut open = None;
    egui::ScrollArea::vertical().id_salt("convs").show(ui, |ui| {
        for c in &state.social.conversations {
            let selected = state.social.open.as_deref() == Some(c.id.as_str());
            let mut label = RichText::new(if c.title.is_empty() { "Conversation" } else { &c.title });
            if c.unread > 0 {
                label = label.strong();
            }
            ui.horizontal(|ui| {
                if ui.selectable_label(selected, label).clicked() {
                    open = Some(c.id.clone());
                }
                if c.unread > 0 {
                    ui.colored_label(widgets::LOSS, format!("{}", c.unread));
                }
            });
            if let Some(m) = &c.last_message {
                ui.weak(truncate(&m.text, 34));
            }
        }
    });
    if let Some(id) = open {
        state.social.open = Some(id);
    }
}

fn open_direct(state: &mut AppState, intents: &IntentTx, friend: &str) {
    let me = state.social.me().map(|m| m.id.clone()).unwrap_or_default();
    let existing = state
        .social
        .conversations
        .iter()
        .find(|c| c.members.len() == 2 && c.members.iter().any(|m| m == friend) && c.members.contains(&me));
    match existing {
        Some(c) => state.social.open = Some(c.id.clone()),
        None => {
            state.social.open_next = true;
            intents.send(Intent::DmCreate {
                members: vec![friend.to_string()],
                name: None,
            });
        }
    }
}

fn conversation(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    let Some(id) = state.social.open.clone() else {
        ui.weak("Pick a friend or conversation.");
        return;
    };
    let Some(conv) = state.social.conversations.iter().find(|c| c.id == id).cloned() else {
        ui.weak("Loading…");
        return;
    };
    // First time we look at a conversation, load its latest page.
    if !state.social.messages.contains_key(&id) {
        state.social.messages.insert(id.clone(), Vec::new());
        intents.send(Intent::DmOpen(id.clone()));
    }
    // Mark read while open.
    if conv.unread > 0 {
        if let Some(last) = state.social.messages.get(&id).and_then(|m| m.last()) {
            intents.send(Intent::DmMarkRead {
                conversation_id: id.clone(),
                message_id: last.id,
            });
        }
        if let Some(c) = state.social.conversations.iter_mut().find(|c| c.id == id) {
            c.unread = 0;
        }
    }

    ui.horizontal(|ui| {
        ui.label(RichText::new(&conv.title).strong().size(16.0));
        if conv.is_group {
            ui.add(
                egui::TextEdit::singleline(&mut state.social.rename)
                    .hint_text("Rename group")
                    .desired_width(140.0),
            );
            if ui.small_button("Rename").clicked() && !state.social.rename.trim().is_empty() {
                intents.send(Intent::DmRename {
                    conversation_id: id.clone(),
                    name: state.social.rename.trim().into(),
                });
                state.social.rename.clear();
            }
        }
    });
    ui.separator();

    let msgs = state.social.messages.get(&id).cloned().unwrap_or_default();
    let input_h = 64.0;
    egui::ScrollArea::vertical()
        .id_salt(("msgs", &id))
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .max_height((ui.available_height() - input_h).max(160.0))
        .show(ui, |ui| {
            if state.social.has_more.get(&id).copied().unwrap_or(false) {
                if let Some(first) = msgs.first() {
                    if ui.small_button("Load older").clicked() {
                        intents.send(Intent::DmLoadOlder {
                            conversation_id: id.clone(),
                            before: first.id,
                        });
                    }
                }
            }
            if msgs.is_empty() {
                ui.weak("No messages yet. Say hi!");
            }
            for m in &msgs {
                let who = state.social.user_name(&m.sender_id);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{who}:")).strong());
                    ui.label(&m.text);
                    ui.weak(widgets::fmt_date(m.sent_at as i64));
                });
            }
        });

    if let Some((who, at)) = state.social.typing.get(&id) {
        if at.elapsed() < TYPING_TTL {
            ui.weak(format!("{} is typing…", state.social.user_name(who)));
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
    }
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut state.social.draft)
                .hint_text("Message")
                .desired_width(ui.available_width() - 70.0),
        );
        if r.changed() && !state.social.draft.is_empty() {
            let due = state
                .social
                .last_typing_sent
                .is_none_or(|t| t.elapsed() > TYPING_SEND_EVERY);
            if due {
                state.social.last_typing_sent = Some(Instant::now());
                intents.send(Intent::DmTyping(id.clone()));
            }
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let text = state.social.draft.trim().to_string();
        if (ui.button("Send").clicked() || enter) && !text.is_empty() {
            intents.send(Intent::DmSend {
                conversation_id: id.clone(),
                text,
            });
            state.social.draft.clear();
            state.social.last_typing_sent = None;
            r.request_focus();
        }
    });
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}
