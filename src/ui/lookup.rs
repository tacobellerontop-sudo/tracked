//! Look up another player by Riot ID and see their profile: rank, headline
//! stats over their recent matches, agents, maps, accolades and history.

use egui::{vec2, Align, Layout, RichText, Sense, Ui};

use super::career;
use super::profile;
use super::theme::{self, pal};
use crate::app::AppState;
use crate::insights;
use crate::ipc::{ClientStatus, Intent, IntentTx};
use crate::riot::models::MatchDetail;
use crate::stats::{self, Range};

const GAP: f32 = 16.0;

pub fn ui(ui: &mut Ui, state: &mut AppState, intents: &IntentTx) {
    let p = pal(ui.ctx());
    let authed = matches!(state.status, ClientStatus::Authed { .. });
    theme::card(ui, None, |ui| {
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut state.lookup.input)
                    .hint_text("Name#TAG")
                    .desired_width(280.0)
                    .margin(egui::Margin::symmetric(10, 7)),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let go = ui
                .add_enabled_ui(authed && !state.lookup.loading, |ui| {
                    theme::accent_button(ui, "Look up")
                })
                .inner
                .clicked();
            if (go || enter) && authed {
                match parse_riot_id(&state.lookup.input) {
                    Some((name, tag)) => {
                        state.lookup.loading = true;
                        state.lookup.result = None;
                        intents.send(Intent::LookupPlayer { name, tag });
                    }
                    None => state.lookup.result = Some(Err("Enter a Riot ID like Name#TAG".into())),
                }
            }
            if state.lookup.loading {
                ui.spinner();
            }
        });
        let hint = if authed {
            "Finds friends, recent teammates and opponents, and lobby players. Click a player on the Live screen to open their profile."
        } else {
            "Start VALORANT to look players up."
        };
        ui.label(RichText::new(hint).size(11.5).color(p.weak));
    });

    let card = match &state.lookup.result {
        None => return,
        Some(Err(e)) => {
            theme::card(ui, None, |ui| {
                ui.label(RichText::new(e).color(p.loss));
            });
            return;
        }
        Some(Ok(card)) => card.clone(),
    };
    let cat = state.catalog.clone();
    let owned: Vec<std::sync::Arc<MatchDetail>> = card
        .matches
        .iter()
        .filter_map(|e| state.matches.get(&e.match_id).cloned())
        .collect();
    let refs: Vec<&MatchDetail> = owned.iter().map(|m| m.as_ref()).collect();
    let o = stats::overview(&card.puuid, &refs);

    // ---- hero
    let (name, tag) = card
        .riot_id
        .as_ref()
        .map(|i| (i.game_name.clone(), format!("#{}", i.tag_line)))
        .unwrap_or_else(|| ("Unknown player".into(), String::new()));
    theme::glow_card(ui, p.accent2, |ui| {
        ui.horizontal(|ui| {
            let initials: String = name.chars().take(2).collect();
            theme::avatar(ui, None, &initials.to_uppercase(), 76.0);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&name).font(theme::semibold(26.0)).color(p.text));
                    ui.label(RichText::new(&tag).size(13.0).color(p.faint));
                });
                let streak = insights::streak(&card.puuid, &refs);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if let Some(perf) = o.me.perf() {
                        theme::chip(ui, profile::perf_label(perf), profile::perf_color(perf, &p));
                    }
                    if streak >= 2 {
                        theme::chip(ui, &format!("{streak} win streak"), p.win);
                    } else if streak <= -2 {
                        theme::chip(ui, &format!("{} loss streak", -streak), p.loss);
                    }
                    if let Some(a) = stats::agents(&card.puuid, &refs).first() {
                        theme::chip(
                            ui,
                            &format!("{} main", profile::agent_name(&cat, &a.agent_id)),
                            p.accent2,
                        );
                    }
                });
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 22.0;
                let r = &card.probe.rank;
                if r.peak_tier > 0 {
                    rank_block(ui, &cat, r.peak_tier, "Peak", None);
                }
                let wr = r.win_rate().map(|w| format!("{w:.0}% WR"));
                let rr = (r.tier > 0).then(|| format!("{} RR", r.rr));
                let sub = [rr, wr].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                rank_block(ui, &cat, r.tier, "Current rank", Some(sub));
            });
        });
    });

    if refs.is_empty() {
        theme::card(ui, Some(110.0), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                if card.matches.is_empty() {
                    ui.label(RichText::new("No recent matches to show stats from.").color(p.weak));
                } else {
                    ui.spinner();
                    ui.label(RichText::new("Loading their matches…").color(p.weak));
                }
            });
        });
    } else {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Statistics").font(theme::semibold(19.0)).color(p.text));
            ui.label(
                RichText::new(format!(
                    "from their last {} matches{}",
                    refs.len(),
                    if refs.len() < card.matches.len() {
                        " (more loading)"
                    } else {
                        ""
                    }
                ))
                .size(12.0)
                .color(p.weak),
            );
        });
        profile::tiles(ui, &profile::overview_tiles(&o, &p), 170.0);

        let w = ui.available_width();
        let two = w >= 860.0;
        let lw = if two { ((w - GAP) * 0.58).floor() } else { w };
        let rw = if two { w - GAP - lw } else { w };
        let agents = stats::agents(&card.puuid, &refs);
        let maps = insights::maps(&card.puuid, &refs);
        let counts = insights::accolade_counts(&card.puuid, &refs);
        let left = |ui: &mut Ui| {
            theme::card(ui, Some(300.0), |ui| {
                theme::card_title(ui, "Performance", None);
                let series = stats::perf_series(&card.puuid, &refs, Range::All, chrono::Utc::now().timestamp());
                profile::perf_chart(ui, 220.0, &series, refs.len() as u64);
            });
            theme::card(ui, None, |ui| {
                theme::card_title(ui, "Maps", None);
                profile::map_rows(ui, &cat, &maps, 6);
            });
        };
        let right = |ui: &mut Ui| {
            theme::card(ui, Some(300.0), |ui| {
                theme::card_title(ui, "Agents", None);
                profile::agent_rows(ui, &cat, &agents, 5);
            });
            theme::card(ui, None, |ui| {
                theme::card_title(ui, "Accolades", None);
                let earned: Vec<_> = counts.iter().filter(|c| c.count > 0).collect();
                if earned.is_empty() {
                    ui.label(RichText::new("No accolades in these matches.").color(p.weak));
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for c in earned {
                        profile::accolade_pill(ui, c.accolade);
                        if c.count > 1 {
                            ui.label(RichText::new(format!("×{}", c.count)).size(11.0).color(p.weak));
                        }
                    }
                });
                ui.add_space(8.0);
                profile::detail_grid(ui, &o.me);
            });
        };
        if two {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                column(ui, lw, left);
                column(ui, rw, right);
            });
        } else {
            left(ui);
            right(ui);
        }
    }

    theme::card(ui, None, |ui| {
        theme::card_title(ui, "Recent matches", None);
        if let Some(action) = career::match_table(ui, state, &card.puuid, &card.matches, "lookup") {
            action.apply(state, Some(card.puuid.clone()), intents);
        }
    });
}

fn column(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        add(ui)
    });
}

/// Rank icon with a label, name and optional sub line (right-to-left layout).
fn rank_block(ui: &mut Ui, cat: &crate::content::Catalog, tier: u32, label: &str, sub: Option<String>) {
    let p = pal(ui.ctx());
    ui.allocate_ui_with_layout(vec2(200.0, 56.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_width(200.0);
        {
            let (r, _) = ui.allocate_exact_size(vec2(46.0, 46.0), Sense::hover());
            match cat.tier(tier).map(|t| t.icon.as_str()).filter(|i| !i.is_empty()) {
                Some(icon) => {
                    egui::Image::new(icon.to_string())
                        .fit_to_exact_size(r.size())
                        .paint_at(ui, r);
                }
                None => {
                    ui.painter().circle_filled(r.center(), 21.0, p.well);
                    super::icons::paint(ui.painter(), r.shrink(13.0), p.weak, super::icons::Icon::Ranking);
                }
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                theme::eyebrow(ui, label);
                ui.label(
                    RichText::new(cat.tier_name(tier))
                        .font(theme::semibold(16.0))
                        .color(p.text),
                );
                if let Some(s) = sub.filter(|s| !s.is_empty()) {
                    ui.label(RichText::new(s).size(11.0).color(p.weak));
                }
            });
        }
    });
}

/// "Name#TAG" -> ("Name", "TAG"). Names may contain spaces; tags may not.
pub fn parse_riot_id(s: &str) -> Option<(String, String)> {
    let (name, tag) = s.trim().rsplit_once('#')?;
    let (name, tag) = (name.trim(), tag.trim());
    let ok = !name.is_empty() && name.chars().count() <= 16 && (3..=5).contains(&tag.chars().count());
    ok.then(|| (name.to_string(), tag.to_string()))
}

#[cfg(test)]
mod tests {
    use super::parse_riot_id;

    #[test]
    fn riot_ids() {
        assert_eq!(parse_riot_id("Some Name#EUW"), Some(("Some Name".into(), "EUW".into())));
        assert_eq!(parse_riot_id(" a#b#1234 "), Some(("a#b".into(), "1234".into())));
        assert_eq!(parse_riot_id("noTag"), None);
        assert_eq!(parse_riot_id("#123"), None);
        assert_eq!(parse_riot_id("x#toolong"), None);
    }
}
