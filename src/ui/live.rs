//! Live game: state banner and the "scout your lobby" roster.

use egui::RichText;
use egui_extras::{Column, TableBuilder};

use super::widgets;
use crate::app::AppState;
use crate::content::valorant_api::queue_name;
use crate::ipc::{ClientStatus, Intent, IntentTx};
use crate::riot::models::{LiveMatch, LivePhase, LivePlayer};
use crate::riot::presence::LoopState;

pub fn loop_state_label(s: LoopState) -> &'static str {
    match s {
        LoopState::Menus => "In menus",
        LoopState::Pregame => "Agent select",
        LoopState::InGame => "In game",
        LoopState::Unknown => "Unknown state",
    }
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.horizontal(|ui| {
        ui.heading("Live");
        if matches!(state.status, ClientStatus::Authed { .. }) {
            if ui
                .button("Rescan lobby")
                .on_hover_text("Re-check everyone's rank")
                .clicked()
            {
                intents.send(Intent::ProbePartyMmr);
            }
            if ui.button("Refresh").clicked() {
                intents.send(Intent::RefreshLiveMatch);
            }
        }
    });
    ui.add_space(6.0);
    if !matches!(state.status, ClientStatus::Authed { .. }) {
        ui.label("Start VALORANT to see live match information.");
        return;
    }

    let banner = match (&state.live, &state.game) {
        (Some(m), _) => match m.phase {
            LivePhase::Party => "In menus",
            LivePhase::Pregame => "Agent select",
            LivePhase::InGame => "In game",
        },
        (None, Some(g)) => loop_state_label(g.loop_state),
        (None, None) => "Waiting for game state…",
    };
    ui.label(RichText::new(banner).size(22.0).strong());

    let Some(live) = state.live.clone() else {
        ui.weak("Party and match rosters appear here automatically.");
        return;
    };
    let mut bits = Vec::new();
    if !live.queue_id.is_empty() || live.phase != LivePhase::Party {
        bits.push(queue_name(&live.queue_id).to_string());
    }
    if !live.map_id.is_empty() {
        bits.push(state.catalog.map_name(&live.map_id));
    }
    if let Some(server) = server_name(&live.server) {
        bits.push(server);
    }
    if !bits.is_empty() {
        ui.weak(bits.join(" · "));
    }
    ui.add_space(8.0);

    let mut lookup = None;
    for team in live.teams() {
        let players: Vec<&LivePlayer> = live.players.iter().filter(|p| p.team_id == team).collect();
        let title = match live.phase {
            LivePhase::Party => format!("Your party ({})", players.len()),
            LivePhase::Pregame => "Your team".to_string(),
            LivePhase::InGame if players.iter().any(|p| p.is_self) => format!("Your team — {team}"),
            LivePhase::InGame => format!("Enemy team — {team}"),
        };
        ui.label(RichText::new(title).strong());
        if let Some(p) = roster(ui, state, &live, &team, &players) {
            lookup = Some(p);
        }
        ui.add_space(10.0);
    }
    if let Some(p) = lookup {
        state.lookup_puuid(&p, intents);
    }
}

/// "aresriot.aws-rclusterprod-euc1-1.eu-gp-frankfurt-1" -> "Frankfurt".
fn server_name(pod: &str) -> Option<String> {
    let last = pod.rsplit('.').next()?;
    let city = last
        .split('-')
        .rev()
        .find(|s| s.chars().all(|c| c.is_ascii_alphabetic()) && s.len() > 2)?;
    let mut c = city.chars();
    Some(c.next()?.to_uppercase().chain(c).collect())
}

fn roster(
    ui: &mut egui::Ui,
    state: &AppState,
    live: &LiveMatch,
    team: &str,
    players: &[&LivePlayer],
) -> Option<String> {
    let cat = state.catalog.clone();
    let mut clicked = None;
    ui.style_mut().interaction.selectable_labels = false;
    TableBuilder::new(ui)
        .id_salt(("roster", team))
        .striped(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(32.0))
        .column(Column::auto().at_least(90.0))
        .column(Column::auto().at_least(170.0))
        .column(Column::exact(28.0))
        .column(Column::auto().at_least(130.0))
        .column(Column::auto().at_least(80.0))
        .column(Column::auto().at_least(110.0))
        .column(Column::auto().at_least(110.0))
        .column(Column::auto().at_least(50.0))
        .header(22.0, |mut h| {
            for t in ["", "Agent", "Player", "", "Rank", "Win %", "Last 5", "Peak", "Level"] {
                h.col(|ui| {
                    ui.strong(t);
                });
            }
        })
        .body(|mut body| {
            for p in players {
                body.row(32.0, |mut row| {
                    row.col(|ui| {
                        if !p.character_id.is_empty() {
                            widgets::agent_icon(ui, &cat, &p.character_id, 28.0);
                        }
                    });
                    row.col(|ui| {
                        let name = cat.agent(&p.character_id).map(|a| a.name.clone());
                        match (name, live.phase) {
                            (Some(n), LivePhase::Pregame) if !p.locked => ui.weak(format!("{n}?")),
                            (Some(n), _) => ui.label(n),
                            (None, LivePhase::Party) => ui.weak("—"),
                            (None, _) => ui.weak("picking…"),
                        };
                    });
                    row.col(|ui| {
                        if p.incognito {
                            ui.weak("Hidden (streamer mode)");
                        } else {
                            let name = state.names.get(&p.puuid).map(|n| n.to_string());
                            let text = name.unwrap_or_else(|| "…".into());
                            if p.is_self {
                                ui.label(RichText::new(text).strong());
                            } else {
                                ui.label(text);
                            }
                        }
                    });
                    let probe = state.probes.get(&p.puuid);
                    match probe {
                        None => {
                            row.col(|_| {});
                            row.col(|ui| {
                                ui.spinner();
                            });
                            for _ in 0..3 {
                                row.col(|_| {});
                            }
                        }
                        Some(Err(e)) => {
                            row.col(|_| {});
                            row.col(|ui| {
                                ui.weak("unavailable").on_hover_text(e);
                            });
                            for _ in 0..3 {
                                row.col(|_| {});
                            }
                        }
                        Some(Ok(pr)) => {
                            row.col(|ui| {
                                widgets::rank_icon(ui, &cat, pr.rank.tier, 24.0);
                            });
                            row.col(|ui| {
                                let mut t = cat.tier_name(pr.rank.tier);
                                if pr.rank.tier > 0 {
                                    t.push_str(&format!(" · {} RR", pr.rank.rr));
                                }
                                ui.label(t);
                            });
                            row.col(|ui| {
                                match pr.rank.win_rate() {
                                    Some(wr) => ui.label(format!("{wr:.0}% ({})", pr.rank.season_games)),
                                    None => ui.weak("—"),
                                };
                            });
                            row.col(|ui| {
                                form_dots(ui, &pr.recent_rr);
                            });
                            row.col(|ui| {
                                if pr.rank.peak_tier > 0 {
                                    ui.label(cat.tier_name(pr.rank.peak_tier));
                                } else {
                                    ui.weak("—");
                                }
                            });
                        }
                    }
                    row.col(|ui| {
                        match p.account_level {
                            Some(l) => ui.label(l.to_string()),
                            None => ui.weak("hidden"),
                        };
                    });
                    // Streamer-mode players can't be looked up by clicking.
                    if row.response().clicked() && !p.incognito {
                        clicked = Some(p.puuid.clone());
                    }
                });
            }
        });
    clicked
}

/// Small win/loss squares for recent RR changes (newest on the right).
fn form_dots(ui: &mut egui::Ui, rr: &[i32]) {
    if rr.is_empty() {
        ui.weak("—");
        return;
    }
    ui.spacing_mut().item_spacing.x = 3.0;
    for r in rr.iter().rev() {
        let color = match r.cmp(&0) {
            std::cmp::Ordering::Greater => widgets::WIN,
            std::cmp::Ordering::Less => widgets::LOSS,
            std::cmp::Ordering::Equal => widgets::DRAW,
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, color);
        resp.on_hover_text(format!("{r:+} RR"));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn server_names() {
        assert_eq!(
            super::server_name("aresriot.aws-rclusterprod-euc1-1.eu-gp-frankfurt-1").as_deref(),
            Some("Frankfurt")
        );
        assert_eq!(super::server_name(""), None);
    }
}
