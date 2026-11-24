//! Recaps: a recap of each play session (games, record, RR, performance,
//! highlights and accolades) and your overall stats across every loaded
//! match.

use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, Ui};

use super::icons::{self, Icon};
use super::profile::{self, perf_badge, perf_color, perf_label};
use super::theme::{self, pal, Palette};
use crate::app::AppState;
use crate::content::Catalog;
use crate::insights::{self, Session};
use crate::ipc::{ClientStatus, IntentTx};
use crate::riot::models::{MatchDetail, Outcome};
use crate::stats::{self, Range};

const GAP: f32 = 16.0;

pub fn ui(ui: &mut Ui, state: &mut AppState, intents: &IntentTx) {
    let p = pal(ui.ctx());
    let Some(career) = state.career.clone() else {
        empty(ui, state);
        return;
    };
    let owned = profile::matches_of(state, &career.puuid);
    let refs: Vec<&MatchDetail> = owned.iter().map(|m| m.as_ref()).collect();
    let sessions = insights::sessions(&career.puuid, &refs, &career.rr_history);
    if sessions.is_empty() {
        theme::card(ui, Some(140.0), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(26.0);
                ui.spinner();
                ui.label(RichText::new("Loading your matches…").color(p.weak));
            });
        });
        return;
    }
    let cat = state.catalog.clone();
    state.recap_session = state.recap_session.min(sessions.len() - 1);

    // ---- session picker
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new("SESSION")
                .font(theme::semibold(10.0))
                .extra_letter_spacing(1.0)
                .color(p.faint),
        );
        ui.add_space(4.0);
        let now = chrono::Utc::now().timestamp();
        for (i, s) in sessions.iter().enumerate().take(8) {
            if theme::pill(ui, &session_label(s.start, now), state.recap_session == i)
                .on_hover_text(format!("{} games", s.matches.len()))
                .clicked()
            {
                state.recap_session = i;
            }
        }
    });

    let s = &sessions[state.recap_session];
    let by_id = |id: &str| owned.iter().find(|m| m.info.match_id == id).map(|m| m.as_ref());
    session_hero(ui, &cat, s, &p);

    // ---- games + highlights
    let w = ui.available_width();
    let two = w >= 860.0;
    let lw = if two { ((w - GAP) * 0.6).floor() } else { w };
    let rw = if two { w - GAP - lw } else { w };
    let mut open = None;
    let games = |ui: &mut Ui, open: &mut Option<String>| {
        theme::card(ui, None, |ui| {
            theme::card_title(ui, "Games this session", None);
            for id in s.matches.iter().rev() {
                if let Some(m) = by_id(id) {
                    if game_row(ui, &cat, &career.puuid, m).clicked() {
                        *open = Some(id.clone());
                    }
                }
            }
        });
    };
    let highlights = |ui: &mut Ui, open: &mut Option<String>| {
        theme::card(ui, None, |ui| {
            theme::card_title(ui, "Highlights", None);
            if let Some((perf, id)) = &s.best {
                if let Some(m) = by_id(id) {
                    if best_game(ui, &cat, &career.puuid, m, *perf).clicked() {
                        *open = Some(id.clone());
                    }
                }
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new("Accolades earned")
                    .font(theme::semibold(12.5))
                    .color(p.text),
            );
            ui.add_space(2.0);
            if s.accolades.is_empty() {
                ui.label(RichText::new("None this session. Next time!").size(12.0).color(p.weak));
            } else {
                let mut counts: Vec<(insights::Accolade, u32)> = Vec::new();
                for a in &s.accolades {
                    match counts.iter_mut().find(|(x, _)| x == a) {
                        Some(e) => e.1 += 1,
                        None => counts.push((*a, 1)),
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for (a, n) in counts {
                        profile::accolade_pill(ui, a);
                        if n > 1 {
                            ui.label(RichText::new(format!("×{n}")).size(11.0).color(p.weak));
                        }
                    }
                });
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Agents played").font(theme::semibold(12.5)).color(p.text));
            ui.horizontal_wrapped(|ui| {
                for (agent, n) in &s.agents {
                    let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                    theme::image_cover(ui, r, cat.agent(agent).map(|a| a.icon.as_str()), 1.0, 10);
                    if cat.agent(agent).is_none_or(|a| a.icon.is_empty()) {
                        profile::initials(ui.painter(), r, &profile::agent_name(&cat, agent), &p);
                    }
                    resp.on_hover_text(format!("{} · {n} games", profile::agent_name(&cat, agent)));
                }
            });
        });
    };
    if two {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            column(ui, lw, |ui| games(ui, &mut open));
            column(ui, rw, |ui| highlights(ui, &mut open));
        });
    } else {
        games(ui, &mut open);
        highlights(ui, &mut open);
    }
    if let Some(id) = open {
        state.open_match(id, None, intents);
        return;
    }

    // ---- overall
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Overall").font(theme::semibold(19.0)).color(p.text));
        ui.label(
            RichText::new(format!("{} matches across {} sessions", refs.len(), sessions.len()))
                .size(12.0)
                .color(p.weak),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if state.older.loading {
                ui.spinner();
                ui.label(RichText::new("Loading older matches…").size(11.5).color(p.weak));
            } else if !state.older.exhausted {
                if theme::accent_button(ui, "Load 20 older matches").clicked() {
                    state.older.request(intents);
                }
            } else {
                ui.label(RichText::new("All available matches loaded").size(11.5).color(p.faint));
            }
        });
    });
    let o = stats::overview(&career.puuid, &refs);
    profile::tiles(ui, &profile::overview_tiles(&o, &p), 170.0);

    let streak = insights::streak(&career.puuid, &refs);
    theme::card(ui, None, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Performance over time")
                    .font(theme::semibold(15.5))
                    .color(p.text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if streak != 0 {
                    let (txt, c) = if streak > 0 {
                        (format!("{streak} win streak"), p.win)
                    } else {
                        (format!("{} loss streak", -streak), p.loss)
                    };
                    theme::chip(ui, &txt, c);
                }
            });
        });
        ui.add_space(6.0);
        let series = stats::perf_series(&career.puuid, &refs, Range::All, chrono::Utc::now().timestamp());
        profile::perf_chart(ui, 200.0, &series, refs.len() as u64);
    });

    let maps = insights::maps(&career.puuid, &refs);
    let agents = stats::agents(&career.puuid, &refs);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let w = ui.available_width();
        let half = ((w - GAP) / 2.0).floor();
        if w >= 760.0 {
            column(ui, half, |ui| {
                theme::card(ui, None, |ui| {
                    theme::card_title(ui, "Maps", None);
                    profile::map_rows(ui, &cat, &maps, 8);
                });
            });
            column(ui, w - GAP - half, |ui| {
                theme::card(ui, None, |ui| {
                    theme::card_title(ui, "Agents", None);
                    profile::agent_rows(ui, &cat, &agents, 8);
                });
            });
        } else {
            column(ui, w, |ui| {
                theme::card(ui, None, |ui| {
                    theme::card_title(ui, "Maps", None);
                    profile::map_rows(ui, &cat, &maps, 8);
                });
                theme::card(ui, None, |ui| {
                    theme::card_title(ui, "Agents", None);
                    profile::agent_rows(ui, &cat, &agents, 8);
                });
            });
        }
    });
    theme::card(ui, None, |ui| {
        theme::card_title(ui, "Career totals", None);
        profile::detail_grid(ui, &o.me);
    });
}

fn empty(ui: &mut Ui, state: &AppState) {
    let p = pal(ui.ctx());
    theme::card(ui, Some(160.0), |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            if matches!(state.status, ClientStatus::Authed { .. }) {
                ui.spinner();
                ui.label(RichText::new("Loading your matches…").color(p.weak));
            } else {
                ui.label(
                    RichText::new("Start VALORANT to see your recaps")
                        .size(16.0)
                        .color(p.text),
                );
                ui.label(RichText::new("Sessions show up here once the Riot Client is connected.").color(p.weak));
            }
        });
    });
}

fn column(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        add(ui)
    });
}

/// "Today", "Yesterday", "Mon", "Nov 12".
fn session_label(start: i64, now: i64) -> String {
    use chrono::{Local, TimeZone};
    let (Some(d), Some(n)) = (
        Local.timestamp_opt(start, 0).single(),
        Local.timestamp_opt(now, 0).single(),
    ) else {
        return "Session".into();
    };
    let days = (n.date_naive() - d.date_naive()).num_days();
    match days {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => d.format("%A").to_string(),
        _ => d.format("%b %d").to_string(),
    }
}

fn session_hero(ui: &mut Ui, cat: &Catalog, s: &Session, p: &Palette) {
    let t = &s.totals;
    let up = t.wins >= t.losses;
    theme::glow_card(ui, if up { p.win } else { p.accent }, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(
                    RichText::new("SESSION RECAP")
                        .font(theme::semibold(10.5))
                        .extra_letter_spacing(1.4)
                        .color(p.accent),
                );
                let when = chrono::DateTime::from_timestamp(s.start, 0)
                    .map(|d| d.with_timezone(&chrono::Local).format("%A, %b %d · %H:%M").to_string())
                    .unwrap_or_default();
                ui.label(RichText::new(when).font(theme::semibold(20.0)).color(p.text));
                ui.label(
                    RichText::new(format!(
                        "{} games · {} played",
                        t.matches,
                        profile::fmt_play_time(t.play_ms)
                    ))
                    .size(12.5)
                    .color(p.weak),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // The session's games as W/L blocks.
                ui.spacing_mut().item_spacing.x = 4.0;
                let _ = cat;
                for (i, perf) in s.perf.iter().enumerate().rev() {
                    let (r, resp) = ui.allocate_exact_size(vec2(10.0, 34.0), Sense::hover());
                    let h = 8.0 + 26.0 * (*perf as f32 / stats::PERF_MAX);
                    let bar = Rect::from_min_max(pos2(r.left(), r.bottom() - h), r.max);
                    ui.painter()
                        .rect_filled(bar, CornerRadius::same(3), perf_color(*perf as f32, p));
                    resp.on_hover_text(format!("Game {} · {perf}", i + 1));
                }
            });
        });
        ui.add_space(14.0);
        let record = format!(
            "{}–{}{}",
            t.wins,
            t.losses,
            if t.draws > 0 {
                format!("–{}", t.draws)
            } else {
                String::new()
            }
        );
        let rr = s.rr.map(|r| format!("{r:+}")).unwrap_or_else(|| "—".into());
        let rr_color = match s.rr {
            Some(r) if r > 0 => p.win,
            Some(r) if r < 0 => p.loss,
            _ => p.text,
        };
        let perf = t.perf();
        let items: Vec<(&str, String, Color32, String)> = vec![
            (
                "Record",
                record,
                if up { p.win } else { p.loss },
                format!("{:.0}% win rate", t.win_rate().unwrap_or(0.0)),
            ),
            ("RR", rr, rr_color, "net ranked rating".into()),
            (
                "Performance",
                perf.map(|v| format!("{v:.0}")).unwrap_or_else(|| "—".into()),
                perf.map(|v| perf_color(v, p)).unwrap_or(p.text),
                perf.map(|v| perf_label(v).to_string()).unwrap_or_default(),
            ),
            (
                "K / D / A",
                format!("{} / {} / {}", t.kills, t.deaths, t.assists),
                p.text,
                format!("{:.2} K/D", t.kd()),
            ),
            (
                "Headshot %",
                format!("{:.0}%", t.hs_pct()),
                p.text,
                format!("{:.0} damage / round", t.adr()),
            ),
        ];
        theme::tiles(ui, &items, 130.0, 12.0, |ui, (label, value, color, sub)| {
            egui::Frame::NONE
                .fill(p.well.gamma_multiply(0.55))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(egui::Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;
                    theme::eyebrow(ui, label);
                    ui.label(RichText::new(value).font(theme::semibold(22.0)).color(*color));
                    ui.label(RichText::new(sub).size(11.0).color(p.weak));
                });
        });
    });
}

/// One game in the session list. Clickable.
fn game_row(ui: &mut Ui, cat: &Catalog, puuid: &str, m: &MatchDetail) -> egui::Response {
    let p = pal(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(12), p.selected);
    }
    let (label, color, a, b) = match m.outcome_for(puuid) {
        Some((Outcome::Win, a, b)) => ("WIN", p.win, a, b),
        Some((Outcome::Loss, a, b)) => ("LOSS", p.loss, a, b),
        Some((Outcome::Draw, a, b)) => ("DRAW", p.weak, a, b),
        None => ("—", p.weak, 0, 0),
    };
    let painter = ui.painter();
    painter.rect_filled(
        Rect::from_min_size(rect.min + vec2(0.0, 10.0), vec2(3.0, rect.height() - 20.0)),
        CornerRadius::same(2),
        color,
    );
    let me = m.player(puuid);
    let agent_r = Rect::from_min_size(rect.min + vec2(12.0, 11.0), vec2(36.0, 36.0));
    let agent = me.map(|x| x.character_id.as_str()).unwrap_or_default();
    theme::image_cover(ui, agent_r, cat.agent(agent).map(|a| a.icon.as_str()), 1.0, 10);
    if cat.agent(agent).is_none_or(|a| a.icon.is_empty()) {
        profile::initials(ui.painter(), agent_r, &profile::agent_name(cat, agent), &p);
    }
    let painter = ui.painter();
    let x = agent_r.right() + 12.0;
    painter.text(
        pos2(x, rect.top() + 12.0),
        Align2::LEFT_TOP,
        cat.map_name(&m.info.map_id),
        theme::semibold(13.0),
        p.text,
    );
    painter.text(
        pos2(x, rect.top() + 31.0),
        Align2::LEFT_TOP,
        format!("{label} {a}–{b}"),
        theme::medium(11.5),
        color,
    );
    if let Some(me) = me {
        let s = &me.stats;
        let kx = rect.left() + rect.width() * 0.42;
        painter.text(
            pos2(kx, rect.top() + 12.0),
            Align2::LEFT_TOP,
            format!("{} / {} / {}", s.kills, s.deaths, s.assists),
            theme::semibold(13.0),
            p.text,
        );
        painter.text(
            pos2(kx, rect.top() + 31.0),
            Align2::LEFT_TOP,
            format!("{:.0} ADR · {:.0}% HS", s.adr(), s.hs_pct()),
            FontId::proportional(11.0),
            p.weak,
        );
    }
    // Rarest accolade as a little medal, then the performance badge.
    let acc = insights::accolades(m, puuid);
    let best = acc.iter().max_by_key(|a| a.rarity());
    let badge = Rect::from_min_size(pos2(rect.right() - 56.0, rect.center().y - 14.0), vec2(48.0, 28.0));
    perf_badge(painter, badge, stats::performance(m, puuid), &p);
    if let Some(a) = best {
        let c = pos2(badge.left() - 22.0, rect.center().y);
        profile::medal(painter, c, 12.0, *a, true, &p);
        let hover = Rect::from_center_size(c, vec2(26.0, 26.0));
        if ui.rect_contains_pointer(hover) {
            let names: Vec<&str> = acc.iter().map(|a| a.name()).collect();
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), resp.id.with("acc"), |ui| {
                ui.label(names.join(" · "));
            });
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn best_game(ui: &mut Ui, cat: &Catalog, puuid: &str, m: &MatchDetail, perf: u32) -> egui::Response {
    let p = pal(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 92.0), Sense::click());
    theme::image_cover(
        ui,
        rect,
        cat.map(&m.info.map_id).map(|x| x.splash.as_str()),
        16.0 / 9.0,
        14,
    );
    let painter = ui.painter();
    // Darken the art so text stays readable.
    let shade = Color32::from_black_alpha(if p.dark { 150 } else { 110 });
    painter.rect_filled(rect, CornerRadius::same(14), shade);
    painter.rect_stroke(
        rect,
        CornerRadius::same(14),
        Stroke::new(1.0_f32, perf_color(perf as f32, &p).gamma_multiply(0.6)),
        egui::StrokeKind::Inside,
    );
    let at = rect.min + vec2(14.0, 12.0);
    icons::paint(painter, Rect::from_min_size(at, vec2(13.0, 13.0)), p.gold, Icon::Star);
    painter.text(
        at + vec2(19.0, 0.0),
        Align2::LEFT_TOP,
        "BEST GAME",
        theme::semibold(10.0),
        p.gold,
    );
    let me = m.player(puuid);
    let agent = me
        .map(|x| profile::agent_name(cat, &x.character_id))
        .unwrap_or_default();
    painter.text(
        rect.min + vec2(14.0, 32.0),
        Align2::LEFT_TOP,
        format!("{} · {agent}", cat.map_name(&m.info.map_id)),
        theme::semibold(15.0),
        Color32::WHITE,
    );
    if let Some(me) = me {
        let s = &me.stats;
        painter.text(
            rect.min + vec2(14.0, 56.0),
            Align2::LEFT_TOP,
            format!("{} / {} / {} · {:.0} ADR", s.kills, s.deaths, s.assists, s.adr()),
            FontId::proportional(12.0),
            Color32::from_white_alpha(210),
        );
    }
    painter.text(
        rect.right_center() - vec2(16.0, 0.0),
        Align2::RIGHT_CENTER,
        perf.to_string(),
        theme::semibold(28.0),
        perf_color(perf as f32, &p),
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The Maps tab of Matches: every map you've played, best first.
pub fn maps(ui: &mut Ui, state: &mut AppState) {
    let p = pal(ui.ctx());
    let Some(career) = state.career.clone() else {
        empty(ui, state);
        return;
    };
    let owned = profile::matches_of(state, &career.puuid);
    let refs: Vec<&MatchDetail> = owned.iter().map(|m| m.as_ref()).collect();
    let maps = insights::maps(&career.puuid, &refs);
    let cat = state.catalog.clone();
    theme::card(ui, None, |ui| {
        theme::card_title(ui, &format!("Maps · {} loaded matches", refs.len()), None);
        if maps.is_empty() {
            ui.label(RichText::new("Play a match to see your maps.").color(p.weak));
        } else {
            profile::map_rows(ui, &cat, &maps, maps.len());
        }
    });
}
