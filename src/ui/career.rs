//! The signed-in player's rank, RR trend and match history.

use egui::RichText;
use egui_extras::{Column, TableBuilder};

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

/// Match history table from `puuid`'s perspective. Returns a clicked match.
pub fn match_table(
    ui: &mut egui::Ui,
    state: &AppState,
    puuid: &str,
    entries: &[MatchHistoryEntry],
    salt: &str,
) -> Option<RowAction> {
    let cat = state.catalog.clone();
    let mut clicked = None;
    // Selectable labels would swallow the click meant for the row.
    ui.style_mut().interaction.selectable_labels = false;
    TableBuilder::new(ui)
        .id_salt(salt)
        .striped(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(28.0))
        .column(Column::auto().at_least(90.0))
        .column(Column::auto().at_least(80.0))
        .column(Column::auto().at_least(90.0))
        .column(Column::auto().at_least(50.0))
        .column(Column::auto().at_least(56.0))
        .columns(Column::auto().at_least(56.0), 4)
        .column(Column::exact(70.0))
        .header(22.0, |mut h| {
            for t in [
                "", "Date", "Map", "Mode", "Result", "Score", "K/D/A", "ADR", "HS%", "ACS", "",
            ] {
                h.col(|ui| {
                    ui.strong(t);
                });
            }
        })
        .body(|mut body| {
            for entry in entries {
                let detail = state.matches.get(&entry.match_id);
                let me = detail.and_then(|d| d.player(puuid));
                body.row(28.0, |mut row| {
                    row.col(|ui| {
                        if let Some(p) = me {
                            widgets::agent_icon(ui, &cat, &p.character_id, 24.0);
                        }
                    });
                    row.col(|ui| {
                        ui.label(widgets::fmt_date(entry.game_start_time / 1000));
                    });
                    let (Some(d), Some(p)) = (detail, me) else {
                        row.col(|ui| {
                            ui.weak("loading…");
                        });
                        for _ in 0..8 {
                            row.col(|_| {});
                        }
                        return;
                    };
                    row.col(|ui| {
                        ui.label(cat.map_name(&d.info.map_id));
                    });
                    row.col(|ui| {
                        ui.label(queue_name(&d.info.queue_id));
                    });
                    let outcome = d.outcome_for(puuid);
                    row.col(|ui| {
                        match outcome {
                            Some((Outcome::Win, a, b)) => ui.colored_label(widgets::WIN, format!("W {a}–{b}")),
                            Some((Outcome::Loss, a, b)) => ui.colored_label(widgets::LOSS, format!("L {a}–{b}")),
                            Some((Outcome::Draw, a, b)) => ui.colored_label(widgets::DRAW, format!("D {a}–{b}")),
                            None => ui.label("—"),
                        };
                    });
                    let s = &p.stats;
                    let cells = [
                        s.score.to_string(),
                        format!("{}/{}/{}", s.kills, s.deaths, s.assists),
                        format!("{:.0}", s.adr()),
                        format!("{:.0}%", s.hs_pct()),
                        format!("{:.0}", s.acs()),
                    ];
                    for c in cells {
                        row.col(|ui| {
                            ui.label(c);
                        });
                    }
                    let mut replay = false;
                    row.col(|ui| {
                        let p = super::theme::pal(ui.ctx());
                        replay = ui
                            .add(
                                egui::Button::new(RichText::new("Replay").size(11.0).color(p.accent))
                                    .corner_radius(egui::CornerRadius::same(10))
                                    .min_size(egui::vec2(58.0, 22.0)),
                            )
                            .on_hover_text("Watch this match as a 2D replay")
                            .clicked();
                    });
                    if replay {
                        clicked = Some(RowAction::Replay(entry.match_id.clone()));
                    } else if row.response().clicked() {
                        clicked = Some(RowAction::Open(entry.match_id.clone()));
                    }
                });
            }
        });
    clicked
}
