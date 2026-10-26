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
        if let Some(id) = super::career::match_table(ui, state, &career.puuid, &entries, "all-matches") {
            state.open_match(id, None, intents);
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
