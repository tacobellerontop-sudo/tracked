//! Mastery: a level for every agent you play (earned from games, wins,
//! performance and accolades) and every accolade you've earned.

use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, Ui};

use super::profile::{self, perf_badge, perf_color};
use super::theme::{self, pal, Palette};
use crate::app::{AppState, MasteryTab};
use crate::content::Catalog;
use crate::insights::{self, AgentMastery, Level};
use crate::ipc::{ClientStatus, IntentTx};
use crate::riot::models::MatchDetail;

pub fn ui(ui: &mut Ui, state: &mut AppState, intents: &IntentTx) {
    let p = pal(ui.ctx());
    let Some(career) = state.career.clone() else {
        theme::card(ui, Some(150.0), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                if matches!(state.status, ClientStatus::Authed { .. }) {
                    ui.spinner();
                } else {
                    ui.label(
                        RichText::new("Start VALORANT to see your mastery")
                            .size(16.0)
                            .color(p.text),
                    );
                }
            });
        });
        return;
    };
    let owned = profile::matches_of(state, &career.puuid);
    let refs: Vec<&MatchDetail> = owned.iter().map(|m| m.as_ref()).collect();
    let cat = state.catalog.clone();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if state.older.loading {
                ui.spinner();
            } else if !state.older.exhausted
                && theme::accent_button(ui, "Load older matches")
                    .on_hover_text("More matches, more mastery")
                    .clicked()
            {
                state.older.request(intents);
            }
            ui.label(
                RichText::new(format!("From {} loaded matches", refs.len()))
                    .size(11.5)
                    .color(p.faint),
            );
        });
    });

    match state.mastery_tab {
        MasteryTab::Agents => agents_tab(ui, state, intents, &cat, &career.puuid, &refs),
        MasteryTab::Accolades => accolades_tab(ui, state, intents, &cat, &career.puuid, &refs),
    }
}

// ------------------------------------------------------------------ agents

/// 12,345 MP.
fn fmt_mp(v: u32) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn agents_tab(
    ui: &mut Ui,
    state: &mut AppState,
    intents: &IntentTx,
    cat: &Catalog,
    puuid: &str,
    refs: &[&MatchDetail],
) {
    let p = pal(ui.ctx());
    let all = insights::mastery(puuid, refs);
    if all.is_empty() {
        theme::card(ui, Some(120.0), |ui| {
            ui.label(RichText::new("Play a match to start earning mastery.").color(p.weak));
        });
        return;
    }
    let selected = state
        .mastery_agent
        .clone()
        .and_then(|id| all.iter().find(|a| a.agent_id == id).cloned())
        .unwrap_or_else(|| all[0].clone());

    if let Some(open) = agent_hero(ui, cat, &selected, &p) {
        state.open_match(open, None, intents);
        return;
    }

    let mut clicked = None;
    theme::tiles(ui, &all, 250.0, 14.0, |ui, a| {
        if agent_card(ui, cat, a, a.agent_id == selected.agent_id, &p).clicked() {
            clicked = Some(a.agent_id.clone());
        }
    });
    if let Some(id) = clicked {
        state.mastery_agent = Some(id);
    }

    theme::card(ui, None, |ui| {
        theme::card_title(ui, "How mastery works", None);
        let rules = [
            ("104 MP / min", "for a win"),
            (
                "80 MP / min",
                "for a loss, and for modes outside Competitive, Unrated, Swiftplay, Spike Rush, TDM and Premier",
            ),
            ("120 MP / min", "for the match MVP in Competitive"),
        ];
        for (pts, what) in rules {
            ui.horizontal(|ui| {
                ui.label(RichText::new(pts).font(theme::semibold(12.5)).color(p.gold));
                ui.label(RichText::new(what).size(12.0).color(p.weak));
            });
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "This follows VALORANT's Agent Mastery: Act levels reset every act (level 1 at 2,000 MP, \
                 level 10 at 132,400 MP, rewards at 4, 7 and 10), and only games since Patch 13.06 count. \
                 Riot doesn't share mastery through its API, so Tracked adds it up from the matches it has \
                 loaded. If your game shows more, load older matches to fill in the rest of this act.",
            )
            .size(11.5)
            .color(p.faint),
        );
    });
}

fn level_line(l: &Level) -> String {
    let t = l.tier();
    if t.is_empty() {
        format!("Act level {}", l.level)
    } else {
        format!("Act level {} · {t}", l.level)
    }
}

pub fn tier_color(level: &Level, p: &Palette) -> Color32 {
    match level.tier_index() {
        0 => Color32::from_rgb(78, 168, 255),
        1 => p.win,
        2 => p.accent2,
        _ => p.gold,
    }
}

/// The selected agent, large. Returns a match id when "best game" is clicked.
fn agent_hero(ui: &mut Ui, cat: &Catalog, a: &AgentMastery, p: &Palette) -> Option<String> {
    let tint = tier_color(&a.level, p);
    let mut open = None;
    theme::glow_card(ui, tint, |ui| {
        let agent = cat.agent(&a.agent_id);
        ui.horizontal_top(|ui| {
            let (art, _) = ui.allocate_exact_size(vec2(112.0, 112.0), Sense::hover());
            ui.painter()
                .rect_filled(art, CornerRadius::same(22), tint.gamma_multiply(0.12));
            let portrait = agent.map(|x| x.portrait.as_str()).filter(|x| !x.is_empty());
            match portrait {
                Some(uri) => {
                    egui::Image::new(uri.to_string())
                        .fit_to_exact_size(art.size())
                        .paint_at(ui, art);
                }
                None => {
                    theme::image_cover(ui, art.shrink(12.0), agent.map(|x| x.icon.as_str()), 1.0, 18);
                    if agent.is_none_or(|x| x.icon.is_empty()) {
                        profile::initials(ui.painter(), art, &profile::agent_name(cat, &a.agent_id), p);
                    }
                }
            }
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                let role = agent.map(|x| x.role.clone()).unwrap_or_default();
                ui.label(
                    RichText::new(if role.is_empty() {
                        "AGENT".into()
                    } else {
                        role.to_uppercase()
                    })
                    .font(theme::semibold(10.5))
                    .extra_letter_spacing(1.4)
                    .color(tint),
                );
                ui.label(
                    RichText::new(profile::agent_name(cat, &a.agent_id))
                        .font(theme::semibold(28.0))
                        .color(p.text),
                );
                ui.horizontal(|ui| {
                    level_emblem(ui, &a.level, 34.0, p);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(
                            RichText::new(level_line(&a.level))
                                .font(theme::semibold(14.0))
                                .color(p.text),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} / {} MP to level {} · {} MP lifetime",
                                fmt_mp(a.level.into),
                                fmt_mp(a.level.needed),
                                a.level.level + 1,
                                fmt_mp(a.points)
                            ))
                            .size(11.5)
                            .color(p.weak),
                        );
                    });
                });
                ui.add_space(4.0);
                let w = ui.available_width().min(520.0);
                theme::gradient_bar(ui, a.level.progress(), w, 8.0, tint.gamma_multiply(0.7), tint);
            });
        });
        ui.add_space(14.0);
        let t = &a.totals;
        let cells: Vec<(&str, String, Color32)> = vec![
            ("Games", t.matches.to_string(), p.text),
            (
                "Win rate",
                t.win_rate().map(|v| format!("{v:.0}%")).unwrap_or_else(|| "–".into()),
                p.text,
            ),
            (
                "Performance",
                t.perf()
                    .map(|v| format!("{:.0}", v.round()))
                    .unwrap_or_else(|| "–".into()),
                t.perf().map(|v| perf_color(v, p)).unwrap_or(p.text),
            ),
            ("K/D", format!("{:.2}", t.kd()), p.text),
            ("Damage / round", format!("{:.0}", t.adr()), p.text),
            ("Headshot %", format!("{:.0}%", t.hs_pct()), p.text),
            ("Accolades", a.accolades.to_string(), p.gold),
            ("Time played", profile::fmt_play_time(t.play_ms), p.text),
        ];
        theme::tiles(ui, &cells, 110.0, 10.0, |ui, (k, v, c)| {
            egui::Frame::NONE
                .fill(p.well.gamma_multiply(0.55))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;
                    theme::eyebrow(ui, k);
                    ui.label(RichText::new(v).font(theme::semibold(18.0)).color(*c));
                });
        });
        if !a.maps.is_empty() || a.best.is_some() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                ui.label(RichText::new("Best maps").size(11.5).color(p.weak));
                for (map, w, g) in a.maps.iter().take(4) {
                    theme::chip(
                        ui,
                        &format!("{} {w}–{}", cat.map_name(map), g - w),
                        if *w * 2 >= *g { p.win } else { p.loss },
                    );
                }
                if let Some((perf, id)) = &a.best {
                    ui.add_space(10.0);
                    if theme::link_button(ui, &format!("Best game: {perf}  ")).clicked() {
                        open = Some(id.clone());
                    }
                }
            });
        }
    });
    open
}

/// A small numbered emblem tinted by tier.
fn level_emblem(ui: &mut Ui, level: &Level, size: f32, p: &Palette) {
    let (r, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let c = tier_color(level, p);
    let pts: Vec<egui::Pos2> = (0..6)
        .map(|i| {
            let a = std::f32::consts::FRAC_PI_3 * i as f32 - std::f32::consts::FRAC_PI_2;
            r.center() + vec2(a.cos(), a.sin()) * size / 2.0
        })
        .collect();
    ui.painter().add(egui::Shape::convex_polygon(
        pts.clone(),
        c.gamma_multiply(0.18),
        Stroke::new(1.5_f32, c),
    ));
    ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        level.level.to_string(),
        theme::semibold(size * 0.4),
        c,
    );
}

fn agent_card(ui: &mut Ui, cat: &Catalog, a: &AgentMastery, selected: bool, p: &Palette) -> egui::Response {
    let tint = tier_color(&a.level, p);
    theme::card(ui, Some(150.0), |ui| {
        let top = ui.max_rect();
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(vec2(46.0, 46.0), Sense::hover());
            theme::image_cover(ui, icon, cat.agent(&a.agent_id).map(|x| x.icon.as_str()), 1.0, 12);
            if cat.agent(&a.agent_id).is_none_or(|x| x.icon.is_empty()) {
                profile::initials(ui.painter(), icon, &profile::agent_name(cat, &a.agent_id), p);
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(
                    RichText::new(profile::agent_name(cat, &a.agent_id))
                        .font(theme::semibold(15.0))
                        .color(p.text),
                );
                ui.label(RichText::new(level_line(&a.level)).size(11.5).color(tint));
            });
        });
        ui.add_space(4.0);
        let w = ui.available_width();
        theme::gradient_bar(ui, a.level.progress(), w, 6.0, tint.gamma_multiply(0.6), tint);
        ui.add_space(4.0);
        let t = &a.totals;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            for (k, v) in [
                ("GAMES", t.matches.to_string()),
                (
                    "WIN",
                    t.win_rate().map(|v| format!("{v:.0}%")).unwrap_or_else(|| "–".into()),
                ),
                ("K/D", format!("{:.2}", t.kd())),
            ] {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(
                        RichText::new(k)
                            .font(theme::medium(9.5))
                            .extra_letter_spacing(0.6)
                            .color(p.faint),
                    );
                    ui.label(RichText::new(v).font(theme::semibold(14.0)).color(p.text));
                });
            }
        });
        let badge = Rect::from_min_size(pos2(top.right() - 46.0, top.top()), vec2(46.0, 26.0));
        perf_badge(ui.painter(), badge, t.perf().map(|v| v.round() as u32), p);
        ui.painter().text(
            pos2(top.right(), top.bottom() - 2.0),
            Align2::RIGHT_BOTTOM,
            format!("{} MP this act", fmt_mp(a.act_points)),
            FontId::proportional(10.5),
            p.faint,
        );
    });
    let last = ui.min_rect();
    let r = ui.interact(last, ui.id().with(("mastery-card", &a.agent_id)), Sense::click());
    if selected {
        ui.painter().rect_stroke(
            last,
            CornerRadius::same(18),
            Stroke::new(1.5_f32, tint.gamma_multiply(0.8)),
            egui::StrokeKind::Inside,
        );
    } else if r.hovered() {
        ui.painter().rect_stroke(
            last,
            CornerRadius::same(18),
            Stroke::new(1.0_f32, p.weak.gamma_multiply(0.5)),
            egui::StrokeKind::Inside,
        );
    }
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// --------------------------------------------------------------- accolades

fn accolades_tab(
    ui: &mut Ui,
    state: &mut AppState,
    intents: &IntentTx,
    cat: &Catalog,
    puuid: &str,
    refs: &[&MatchDetail],
) {
    let p = pal(ui.ctx());
    let counts = insights::accolade_counts(puuid, refs);
    let feed = insights::accolade_feed(puuid, refs);
    let total: u32 = counts.iter().map(|c| c.count).sum();
    let kinds = counts.iter().filter(|c| c.count > 0).count();
    let top = counts.iter().filter(|c| c.count > 0).max_by_key(|c| c.count);

    theme::glow_card(ui, p.gold, |ui| {
        let items: Vec<(&str, String, String)> = vec![
            (
                "Accolades earned",
                total.to_string(),
                format!("in {} matches", refs.len()),
            ),
            (
                "Collection",
                format!("{kinds} / {}", insights::Accolade::ALL.len()),
                "different accolades".into(),
            ),
            (
                "Most earned",
                top.map(|c| c.accolade.name().to_string()).unwrap_or_else(|| "–".into()),
                top.map(|c| format!("{} times", c.count)).unwrap_or_default(),
            ),
            (
                "Per match",
                if refs.is_empty() {
                    "–".into()
                } else {
                    format!("{:.1}", total as f32 / refs.len() as f32)
                },
                "on average".into(),
            ),
        ];
        theme::tiles(ui, &items, 150.0, 12.0, |ui, (k, v, sub)| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                theme::eyebrow(ui, k);
                ui.label(RichText::new(v).font(theme::semibold(22.0)).color(p.text));
                ui.label(RichText::new(sub).size(11.0).color(p.weak));
            });
        });
    });

    theme::tiles(ui, &counts, 200.0, 14.0, |ui, c| {
        theme::card(ui, Some(176.0), |ui| {
            let earned = c.count > 0;
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
            profile::medal(ui.painter(), r.center(), 26.0, c.accolade, earned, &p);
            if c.count > 1 {
                let b = Rect::from_center_size(r.center() + vec2(24.0, 20.0), vec2(30.0, 18.0));
                ui.painter().rect_filled(b, CornerRadius::same(9), p.text);
                ui.painter().text(
                    b.center(),
                    Align2::CENTER_CENTER,
                    format!("×{}", c.count),
                    theme::semibold(10.5),
                    p.card,
                );
            }
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new(c.accolade.name())
                        .font(theme::semibold(13.5))
                        .color(if earned { p.text } else { p.weak }),
                );
                ui.label(
                    RichText::new(profile::rarity_name(c.accolade.rarity()).to_uppercase())
                        .font(theme::semibold(9.0))
                        .extra_letter_spacing(1.0)
                        .color(profile::rarity_color(c.accolade.rarity(), &p)),
                );
                ui.label(RichText::new(c.accolade.description()).size(11.0).color(p.weak));
                if let Some((_, at)) = &c.last {
                    ui.label(
                        RichText::new(format!(
                            "Last earned {}",
                            crate::stats::ago(*at, chrono::Utc::now().timestamp())
                        ))
                        .size(10.5)
                        .color(p.faint),
                    );
                }
            });
        });
    });

    let mut open = None;
    theme::card(ui, None, |ui| {
        theme::card_title(ui, "Recently earned", None);
        if feed.is_empty() {
            ui.label(RichText::new("Nothing yet. Accolades show up here after your matches.").color(p.weak));
        }
        let now = chrono::Utc::now().timestamp();
        for e in feed.iter().take(12) {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(10), p.selected);
            }
            profile::medal(
                ui.painter(),
                rect.left_center() + vec2(20.0, 0.0),
                14.0,
                e.accolade,
                true,
                &p,
            );
            ui.painter().text(
                rect.left_center() + vec2(46.0, -8.0),
                Align2::LEFT_CENTER,
                e.accolade.name(),
                theme::semibold(13.0),
                p.text,
            );
            ui.painter().text(
                rect.left_center() + vec2(46.0, 9.0),
                Align2::LEFT_CENTER,
                format!(
                    "{} · {}",
                    cat.map_name(&e.map_id),
                    profile::agent_name(cat, &e.agent_id)
                ),
                FontId::proportional(11.0),
                p.weak,
            );
            ui.painter().text(
                rect.right_center() - vec2(8.0, 0.0),
                Align2::RIGHT_CENTER,
                crate::stats::ago(e.at, now),
                FontId::proportional(11.0),
                p.faint,
            );
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                open = Some(e.match_id.clone());
            }
        }
    });
    if let Some(id) = open {
        state.open_match(id, None, intents);
    }
}
