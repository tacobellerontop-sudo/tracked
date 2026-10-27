//! The signed-in player's rank, RR trend and match history.

use egui::RichText;

use super::widgets;
use crate::app::AppState;
use crate::content::valorant_api::queue_name;
use crate::ipc::{ClientStatus, Intent, IntentTx};
use crate::riot::models::{MatchHistoryEntry, MatchId, Outcome};

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.horizontal(|ui| {
        ui.heading("Career");
        if state.career_loading {
            ui.spinner();
        } else if matches!(state.status, ClientStatus::Authed { .. }) && ui.button("Refresh").clicked() {
            intents.send(Intent::RefreshCareer);
        }
    });
    ui.add_space(6.0);

    let Some(career) = state.career.clone() else {
        if matches!(state.status, ClientStatus::Authed { .. }) {
            ui.label("Loading your rank and matches…");
        } else {
            ui.label("Start VALORANT to see your rank and match history.");
        }
        return;
    };
    let cat = state.catalog.clone();

    ui.horizontal(|ui| {
        widgets::rank_icon(ui, &cat, career.rank.tier, 72.0);
        ui.vertical(|ui| {
            if let Some(id) = &career.riot_id {
                ui.label(RichText::new(id.to_string()).strong());
            }
            ui.label(RichText::new(cat.tier_name(career.rank.tier)).size(22.0).strong());
            if career.rank.tier > 0 {
                ui.label(format!("{} RR", career.rank.rr));
            }
            let mut extra = Vec::new();
            if let Some(wr) = career.rank.win_rate() {
                extra.push(format!(
                    "{wr:.0}% win rate this act ({} games)",
                    career.rank.season_games
                ));
            }
            if career.rank.peak_tier > 0 {
                extra.push(format!("peak {}", cat.tier_name(career.rank.peak_tier)));
            }
            if !extra.is_empty() {
                ui.weak(extra.join(" · "));
            }
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            ui.weak("RR trend (recent competitive)");
            let values: Vec<i32> = career.rr_history.iter().map(|p| p.ladder()).collect();
            widgets::sparkline(ui, &values, egui::vec2(260.0, 64.0));
            if let Some(last) = career.rr_history.last() {
                let sign = if last.earned >= 0 { "+" } else { "" };
                ui.weak(format!("last game {sign}{} RR", last.earned));
            }
        });
    });
    ui.add_space(10.0);
    ui.separator();
    ui.label(RichText::new("Recent matches").strong());

    if let Some(action) = match_table(ui, state, &career.puuid, &career.matches, "history") {
        action.apply(state, None, intents);
    }
}

/// What a click in the match table asked for.
pub enum RowAction {
    Open(MatchId),
    Replay(MatchId),
}

impl RowAction {
    /// Carry the action out, from `perspective`'s point of view (default: you).
    pub fn apply(self, state: &mut AppState, perspective: Option<String>, intents: &IntentTx) {
        match self {
            RowAction::Open(id) => state.open_match(id, perspective, intents),
            RowAction::Replay(id) => state.open_replay(id, perspective, intents),
        }
    }
}

/// Match history from `puuid`'s perspective: one roomy row per match.
/// Returns what was clicked (open the match, or its replay).
pub fn match_table(
    ui: &mut egui::Ui,
    state: &AppState,
    puuid: &str,
    entries: &[MatchHistoryEntry],
    salt: &str,
) -> Option<RowAction> {
    use egui::{pos2, vec2, Align2, Color32, CornerRadius, Rect, Sense};
    let p = super::theme::pal(ui.ctx());
    let cat = state.catalog.clone();
    let mut clicked = None;
    let width = ui.available_width();
    // Drop the least important stats on narrow windows instead of squeezing.
    let stats_shown = if width > 980.0 {
        5
    } else if width > 820.0 {
        4
    } else {
        3
    };
    for entry in entries {
        let detail = state.matches.get(&entry.match_id);
        let me = detail.and_then(|d| d.player(puuid));
        let (rect, row) = ui.allocate_exact_size(vec2(width, 64.0), Sense::click());
        let painter = ui.painter();
        let hovered = row.hovered();
        painter.rect_filled(
            rect,
            CornerRadius::same(12),
            if hovered {
                p.selected
            } else {
                p.well.gamma_multiply(0.55)
            },
        );
        let outcome = detail.and_then(|d| d.outcome_for(puuid));
        let stripe = match outcome {
            Some((Outcome::Win, ..)) => p.win,
            Some((Outcome::Loss, ..)) => p.loss,
            _ => p.faint,
        };
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(4.0, rect.height())),
            CornerRadius {
                nw: 12,
                sw: 12,
                ne: 0,
                se: 0,
            },
            stripe,
        );
        let mid = rect.center().y;
        let mut x = rect.left() + 18.0;

        // Agent.
        let icon = Rect::from_center_size(pos2(x + 20.0, mid), vec2(40.0, 40.0));
        let agent = me.and_then(|m| cat.agent(&m.character_id));
        match agent.map(|a| a.icon.as_str()).filter(|u| !u.is_empty()) {
            Some(uri) => {
                painter.rect_filled(icon, CornerRadius::same(10), p.card);
                egui::Image::new(uri)
                    .corner_radius(CornerRadius::same(10))
                    .paint_at(ui, icon);
            }
            None => {
                painter.rect_filled(icon, CornerRadius::same(10), p.card);
                let letter = agent.and_then(|a| a.name.chars().next()).unwrap_or('?');
                painter.text(
                    icon.center(),
                    Align2::CENTER_CENTER,
                    letter,
                    super::theme::semibold(15.0),
                    p.weak,
                );
            }
        }
        x += 56.0;

        // Map, mode and date.
        let painter = ui.painter();
        let (map, mode) = match detail {
            Some(d) => (cat.map_name(&d.info.map_id), queue_name(&d.info.queue_id).to_string()),
            None => ("Loading…".to_string(), queue_name(&entry.queue_id).to_string()),
        };
        painter.text(
            pos2(x, mid - 2.0),
            Align2::LEFT_BOTTOM,
            map,
            super::theme::semibold(14.0),
            p.text,
        );
        painter.text(
            pos2(x, mid + 3.0),
            Align2::LEFT_TOP,
            format!("{mode} · {}", widgets::fmt_date(entry.game_start_time / 1000)),
            super::theme::medium(11.0),
            p.weak,
        );
        x += 210.0;

        // Result.
        if let Some((o, a, b)) = outcome {
            let (letter, color) = match o {
                Outcome::Win => ("W", p.win),
                Outcome::Loss => ("L", p.loss),
                Outcome::Draw => ("D", p.weak),
            };
            painter.text(
                pos2(x, mid),
                Align2::LEFT_CENTER,
                format!("{letter} {a}–{b}"),
                super::theme::semibold(14.0),
                color,
            );
        }
        x += 90.0;

        // Stats: value over label.
        let mut perf_rect = None;
        if let (Some(d), Some(m)) = (detail, me) {
            let s = &m.stats;
            let perf = crate::stats::performance(d, puuid);
            let perf_color = match perf {
                Some(v) if v >= 600 => p.win,
                Some(v) if v <= 400 => p.loss,
                _ => p.text,
            };
            let all = [
                (
                    "Performance",
                    perf.map(|v| v.to_string()).unwrap_or_else(|| "—".into()),
                    perf_color,
                ),
                (
                    "K / D / A",
                    format!("{} / {} / {}", s.kills, s.deaths, s.assists),
                    p.text,
                ),
                ("ACS", format!("{:.0}", s.acs()), p.text),
                ("ADR", format!("{:.0}", s.adr()), p.text),
                ("HS%", format!("{:.0}%", s.hs_pct()), p.text),
            ];
            for (i, (label, value, color)) in all.into_iter().take(stats_shown).enumerate() {
                let w = if i == 1 { 120.0 } else { 96.0 };
                painter.text(
                    pos2(x, mid - 2.0),
                    Align2::LEFT_BOTTOM,
                    value,
                    super::theme::semibold(14.0),
                    color,
                );
                painter.text(
                    pos2(x, mid + 3.0),
                    Align2::LEFT_TOP,
                    label,
                    super::theme::medium(10.5),
                    p.faint,
                );
                if i == 0 {
                    perf_rect = Some(Rect::from_min_size(pos2(x, mid - 18.0), vec2(w - 10.0, 36.0)));
                }
                x += w;
            }
        }
        let _ = x;

        // Replay button on the right (drawn after the row so it gets the click).
        let btn = Rect::from_center_size(pos2(rect.right() - 52.0, mid), vec2(76.0, 28.0));
        let resp = ui.interact(btn, ui.id().with((salt, "replay", &entry.match_id)), Sense::click());
        let painter = ui.painter();
        painter.rect_filled(
            btn,
            CornerRadius::same(14),
            if resp.hovered() {
                p.accent
            } else {
                p.accent.gamma_multiply(0.14)
            },
        );
        painter.text(
            btn.center(),
            Align2::CENTER_CENTER,
            "Replay",
            super::theme::semibold(12.0),
            if resp.hovered() { Color32::WHITE } else { p.accent },
        );
        let replay = resp.on_hover_text("Watch this match as a 2D replay").clicked();
        if let Some(r) = perf_rect {
            ui.interact(r, ui.id().with((salt, "perf", &entry.match_id)), Sense::hover())
                .on_hover_text(performance_help());
        }
        if replay {
            clicked = Some(RowAction::Replay(entry.match_id.clone()));
        } else if row.clicked() {
            clicked = Some(RowAction::Open(entry.match_id.clone()));
        }
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.add_space(8.0);
    }
    clicked
}

/// Tooltip text explaining the performance score.
pub fn performance_help() -> String {
    let weights: Vec<String> = crate::stats::PERFORMANCE_WEIGHTS
        .iter()
        .map(|(name, w)| format!("{name} {:.0}%", w * 100.0))
        .collect();
    format!(
        "Performance (0–1000): how you played compared with the other players in this match. \
         500 is the lobby average.\nBuilt from {}.",
        weights.join(", ")
    )
}
