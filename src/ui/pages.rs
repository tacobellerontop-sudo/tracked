//! Matches and Agents pages.

use egui::{vec2, Align, Layout, RichText, Sense, Ui};

use super::theme::{self, pal};
use crate::app::AppState;
use crate::content::valorant_api::queue_name;
use crate::ipc::IntentTx;
use crate::stats;

/// Does `query` (from the search bar) match any of these strings?
fn matches_query(query: &str, fields: &[&str]) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || fields.iter().any(|f| f.to_lowercase().contains(&q))
}

pub fn matches(ui: &mut Ui, state: &mut AppState, intents: &IntentTx) {
    let p = pal(ui.ctx());
    let Some(career) = state.career.clone() else {
        theme::card(ui, None, |ui| {
            ui.weak("Start VALORANT to see your match history.");
        });
        return;
    };
    let cat = state.catalog.clone();
    let query = state.search.clone();
    let entries: Vec<_> = career
        .matches
        .iter()
        .filter(|e| match state.matches.get(&e.match_id) {
            Some(d) => {
                let agent = d
                    .player(&career.puuid)
                    .and_then(|me| cat.agent(&me.character_id))
                    .map(|a| a.name.clone());
                matches_query(
                    &query,
                    &[
                        &cat.map_name(&d.info.map_id),
                        queue_name(&d.info.queue_id),
                        agent.as_deref().unwrap_or(""),
                    ],
                )
            }
            None => query.trim().is_empty(),
        })
        .cloned()
        .collect();
    theme::card(ui, None, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Match History").size(15.0).strong().color(p.text));
            ui.label(
                RichText::new(format!("{} matches", entries.len()))
                    .size(11.5)
                    .color(p.faint),
            );
            if !query.trim().is_empty() {
                ui.label(
                    RichText::new(format!("filtered by “{}”", query.trim()))
                        .size(11.5)
                        .color(p.weak),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if state.career_loading {
                    ui.spinner();
                } else if theme::pill(ui, "Refresh", false).clicked() {
                    intents.send(crate::ipc::Intent::RefreshCareer);
                }
            });
        });
        ui.add_space(8.0);
        if let Some(action) = super::career::match_table(ui, state, &career.puuid, &entries, "all-matches") {
            action.apply(state, None, intents);
        }
    });
}

pub fn agents(ui: &mut Ui, state: &mut AppState) {
    let p = pal(ui.ctx());
    let Some(career) = state.career.clone() else {
        theme::card(ui, None, |ui| {
            ui.weak("Start VALORANT to see your agents.");
        });
        return;
    };
    let cat = state.catalog.clone();
    let refs = stats::loaded(career.matches.iter().map(|e| &e.match_id), &state.matches);
    let list: Vec<_> = stats::agents(&career.puuid, &refs)
        .into_iter()
        .filter(|a| {
            let name = cat.agent(&a.agent_id).map(|x| x.name.clone()).unwrap_or_default();
            matches_query(&state.search, &[&name])
        })
        .collect();
    ui.label(
        RichText::new(format!("Agents · last {} matches", refs.len()))
            .size(15.0)
            .strong()
            .color(p.text),
    );
    if list.is_empty() {
        theme::card(ui, None, |ui| {
            ui.weak("No agents to show yet.");
        });
        return;
    }
    let w = ui.available_width();
    let per_row = ((w + 16.0) / 260.0).floor().max(1.0) as usize;
    let cw = (w - 16.0 * (per_row as f32 - 1.0)) / per_row as f32;
    for chunk in list.chunks(per_row) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for a in chunk {
                ui.allocate_ui_with_layout(vec2(cw, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(cw);
                    theme::card(ui, None, |ui| {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                            theme::image_cover(ui, r, cat.agent(&a.agent_id).map(|x| x.icon.as_str()), 1.0, 24);
                            ui.vertical(|ui| {
                                let name = cat
                                    .agent(&a.agent_id)
                                    .map(|x| x.name.clone())
                                    .unwrap_or_else(|| "Unknown".into());
                                ui.label(RichText::new(name).size(16.0).color(p.text));
                                ui.label(
                                    RichText::new(format!("{} matches", a.totals.matches))
                                        .size(11.5)
                                        .color(p.weak),
                                );
                            });
                        });
                        ui.add_space(8.0);
                        let t = &a.totals;
                        let wr = t.win_rate().map(|w| format!("{w:.0}%")).unwrap_or_else(|| "–".into());
                        egui::Grid::new(("agent", &a.agent_id))
                            .num_columns(2)
                            .spacing([24.0, 4.0])
                            .show(ui, |ui| {
                                for (k, v) in [
                                    ("Win rate", wr),
                                    ("K/D", format!("{:.2}", t.kd())),
                                    ("ACS", format!("{:.0}", t.acs())),
                                    ("ADR", format!("{:.0}", t.adr())),
                                    ("Headshot %", format!("{:.1}%", t.hs_pct())),
                                ] {
                                    ui.label(RichText::new(k).size(12.0).color(p.weak));
                                    ui.label(RichText::new(v).size(12.5).color(p.text));
                                    ui.end_row();
                                }
                            });
                    });
                });
            }
        });
        ui.add_space(16.0);
    }
}

/// Replays: recent matches as cards, each with a big "Watch replay" button.
pub fn replays(ui: &mut Ui, state: &mut AppState, intents: &IntentTx) {
    let p = pal(ui.ctx());
    let Some(career) = state.career.clone() else {
        theme::card(ui, None, |ui| {
            ui.weak("Start VALORANT to see your matches and their replays.");
        });
        return;
    };
    let cat = state.catalog.clone();
    let query = state.search.clone();
    let mut open = None;
    theme::card(ui, None, |ui| {
        ui.horizontal(|ui| {
            super::icons::icon(ui, super::icons::Icon::Map, 18.0, p.accent);
            ui.label(RichText::new("Replays").font(theme::semibold(16.0)).color(p.text));
        });
        ui.label(
            RichText::new(
                "Watch any match top-down on the minimap: every player, every kill, the plant and the defuse, \
                 played like a video.",
            )
            .size(12.0)
            .color(p.weak),
        );
        ui.add_space(12.0);
        let gap = 14.0;
        let cols = (((ui.available_width() + gap) / (250.0 + gap)).floor() as usize).max(1);
        let w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let entries: Vec<_> = career
            .matches
            .iter()
            .filter(|e| match state.matches.get(&e.match_id) {
                Some(d) => matches_query(&query, &[&cat.map_name(&d.info.map_id), queue_name(&d.info.queue_id)]),
                None => query.trim().is_empty(),
            })
            .collect();
        if entries.is_empty() {
            ui.weak("No matches yet.");
        }
        for row in entries.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for e in row {
                    let d = state.matches.get(&e.match_id);
                    if replay_card(ui, &cat, &career.puuid, e, d.map(|d| d.as_ref()), w, &p) {
                        open = Some(e.match_id.clone());
                    }
                }
            });
            ui.add_space(gap);
        }
    });
    if let Some(id) = open {
        state.open_replay(id, None, intents);
    }
}

fn replay_card(
    ui: &mut Ui,
    cat: &crate::content::Catalog,
    puuid: &str,
    e: &crate::riot::models::MatchHistoryEntry,
    d: Option<&crate::riot::models::MatchDetail>,
    w: f32,
    p: &theme::Palette,
) -> bool {
    use egui::{pos2, Align2, Color32, CornerRadius, Rect};
    let h = 214.0;
    let (rect, card) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let painter = ui.painter();
    let lift = if card.hovered() { 1.0 } else { 0.0 };
    painter.rect_filled(rect, CornerRadius::same(14), p.well.gamma_multiply(0.6 + 0.4 * lift));
    let img = Rect::from_min_size(rect.min, vec2(w, 92.0));
    let map_id = d.map(|d| d.info.map_id.as_str()).unwrap_or_default();
    let icon = cat.map(map_id).map(|m| m.icon.as_str());
    theme::image_cover(
        ui,
        img,
        icon,
        452.0 / 128.0,
        CornerRadius {
            nw: 14,
            ne: 14,
            sw: 0,
            se: 0,
        },
    );
    let painter = ui.painter();
    // Result badge over the art.
    if let Some((outcome, a, b)) = d.and_then(|d| d.outcome_for(puuid)) {
        use crate::riot::models::Outcome;
        let (label, color) = match outcome {
            Outcome::Win => (format!("W {a}–{b}"), p.win),
            Outcome::Loss => (format!("L {a}–{b}"), p.loss),
            Outcome::Draw => (format!("D {a}–{b}"), p.weak),
        };
        let g = painter.layout_no_wrap(label, theme::semibold(12.0), Color32::WHITE);
        let badge = Rect::from_min_size(img.min + vec2(10.0, 10.0), g.size() + vec2(14.0, 8.0));
        painter.rect_filled(badge, CornerRadius::same(8), color);
        painter.galley(badge.min + vec2(7.0, 4.0), g, Color32::WHITE);
    }
    let x = rect.left() + 14.0;
    let map_name = if map_id.is_empty() {
        "Loading…".to_string()
    } else {
        cat.map_name(map_id)
    };
    painter.text(
        pos2(x, img.bottom() + 12.0),
        Align2::LEFT_TOP,
        map_name,
        theme::semibold(14.0),
        p.text,
    );
    let queue = d
        .map(|d| queue_name(&d.info.queue_id))
        .unwrap_or(queue_name(&e.queue_id));
    let mut sub = format!("{queue} · {}", super::widgets::fmt_date(e.game_start_time / 1000));
    if let Some(me) = d.and_then(|d| d.player(puuid)) {
        let agent = cat.agent(&me.character_id).map(|a| a.name.clone()).unwrap_or_default();
        sub = format!(
            "{sub}\n{agent} · {}/{}/{}",
            me.stats.kills, me.stats.deaths, me.stats.assists
        );
    }
    painter.text(
        pos2(x, img.bottom() + 32.0),
        Align2::LEFT_TOP,
        sub,
        theme::medium(11.5),
        p.weak,
    );

    // Watch button.
    let supported = crate::replay::availability_for(&e.queue_id, "") == crate::replay::Availability::Available;
    let btn = Rect::from_min_max(
        pos2(x, rect.bottom() - 44.0),
        pos2(rect.right() - 14.0, rect.bottom() - 12.0),
    );
    let resp = ui.interact(btn, ui.id().with(("watch", &e.match_id)), Sense::click());
    let painter = ui.painter();
    if supported {
        let fill = if resp.hovered() { p.accent } else { p.navy };
        painter.rect_filled(btn, CornerRadius::same(10), fill);
        let label = painter.layout_no_wrap("Watch replay".into(), theme::semibold(12.5), p.on_navy);
        let total = 16.0 + 8.0 + label.size().x;
        let start = btn.center().x - total / 2.0;
        super::icons::paint(
            painter,
            Rect::from_center_size(pos2(start + 8.0, btn.center().y), vec2(14.0, 14.0)),
            p.on_navy,
            super::icons::Icon::Play,
        );
        painter.galley(
            pos2(start + 24.0, btn.center().y - label.size().y / 2.0),
            label,
            p.on_navy,
        );
    } else {
        painter.rect_filled(btn, CornerRadius::same(10), p.well);
        painter.text(
            btn.center(),
            Align2::CENTER_CENTER,
            "No replay for this mode",
            theme::medium(11.5),
            p.faint,
        );
    }
    if supported && (resp.hovered() || card.hovered()) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    supported && (resp.clicked() || card.clicked())
}
