//! Full scoreboard and round-by-round strip for one match.

use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};

use super::widgets;
use crate::app::{AppState, Screen};
use crate::content::valorant_api::queue_name;
use crate::content::Catalog;
use crate::ipc::IntentTx;
use crate::riot::models::{MatchDetail, PlayerMatch};

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, _intents: &IntentTx) {
    if ui.button("Back").clicked() {
        state.screen = if state.return_to == Screen::MatchDetail {
            Screen::Career
        } else {
            state.return_to
        };
    }
    let Some(id) = state.selected_match.clone() else { return };
    let Some(d) = state.matches.get(&id).cloned() else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Loading match…");
        });
        return;
    };
    let cat = state.catalog.clone();
    let me = state.match_perspective.clone();
    let my_team = me.as_deref().and_then(|p| d.player(p)).map(|p| p.team_id.clone());

    ui.horizontal(|ui| {
        widgets::icon(ui, cat.map(&d.info.map_id).map(|m| m.icon.as_str()), 48.0);
        ui.vertical(|ui| {
            ui.heading(format!(
                "{} · {}",
                cat.map_name(&d.info.map_id),
                queue_name(&d.info.queue_id)
            ));
            ui.weak(format!(
                "{} · {}",
                widgets::fmt_date(d.info.started_at_unix),
                widgets::fmt_duration_ms(d.info.game_length_ms)
            ));
        });
    });
    ui.add_space(6.0);
    round_strip(ui, &d, my_team.as_deref());
    ui.add_space(8.0);

    let mut teams: Vec<&str> = d.teams.iter().map(|t| t.team_id.as_str()).collect();
    if let Some(mine) = &my_team {
        teams.sort_by_key(|t| *t != mine.as_str());
    }
    for team in teams {
        let t = d.team(team);
        let label = format!(
            "{} — {} rounds {}",
            team,
            t.map(|t| t.rounds_won).unwrap_or(0),
            if t.is_some_and(|t| t.won) { "· WIN" } else { "" }
        );
        ui.label(RichText::new(label).strong());
        let mut players: Vec<&PlayerMatch> = d.players.iter().filter(|p| p.team_id == team).collect();
        players.sort_by(|a, b| b.stats.acs().total_cmp(&a.stats.acs()));
        scoreboard(ui, &cat, team, &players, me.as_deref());
        ui.add_space(10.0);
    }
}

fn round_strip(ui: &mut egui::Ui, d: &MatchDetail, my_team: Option<&str>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for r in &d.rounds {
            let color = match my_team {
                Some(t) if r.winning_team == t => widgets::WIN,
                Some(_) => widgets::LOSS,
                None => Color32::GRAY,
            };
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(14.0, 18.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, color);
            resp.on_hover_text(format!(
                "Round {} · {} · {}",
                r.round_num + 1,
                r.winning_team,
                r.round_result
            ));
        }
    });
}

fn scoreboard(ui: &mut egui::Ui, cat: &Catalog, team: &str, players: &[&PlayerMatch], me: Option<&str>) {
    TableBuilder::new(ui)
        .id_salt(team)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(28.0))
        .column(Column::auto().at_least(150.0))
        .column(Column::exact(28.0))
        .columns(Column::auto().at_least(40.0), 11)
        .header(20.0, |mut h| {
            for t in [
                "", "Player", "", "ACS", "K", "D", "A", "K/D", "ADR", "HS%", "KAST", "FK/FD", "3K/4K/5K", "Pl/Df",
            ] {
                h.col(|ui| {
                    ui.strong(t);
                });
            }
        })
        .body(|mut body| {
            for p in players {
                let s = &p.stats;
                body.row(28.0, |mut row| {
                    row.col(|ui| {
                        widgets::agent_icon(ui, cat, &p.character_id, 24.0);
                    });
                    row.col(|ui| {
                        let name = format!("{}#{}", p.game_name, p.tag_line);
                        if Some(p.puuid.as_str()) == me {
                            ui.label(RichText::new(name).strong());
                        } else {
                            ui.label(name);
                        }
                    });
                    row.col(|ui| {
                        widgets::rank_icon(ui, cat, p.competitive_tier, 22.0);
                    });
                    let cells = [
                        format!("{:.0}", s.acs()),
                        s.kills.to_string(),
                        s.deaths.to_string(),
                        s.assists.to_string(),
                        format!("{:.2}", s.kd()),
                        format!("{:.0}", s.adr()),
                        format!("{:.0}%", s.hs_pct()),
                        format!("{:.0}%", s.kast_pct()),
                        format!("{}/{}", s.first_kills, s.first_deaths),
                        format!("{}/{}/{}", s.multi_kill_rounds3, s.multi_kill_rounds4, s.aces),
                        format!("{}/{}", s.plants, s.defuses),
                    ];
                    for c in cells {
                        row.col(|ui| {
                            ui.label(c);
                        });
                    }
                });
            }
        });
}
