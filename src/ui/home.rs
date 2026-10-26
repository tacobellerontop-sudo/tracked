//! The Home dashboard: profile banner, headline stats, performance chart,
//! top agents, rank progression, recent activity, match history, current
//! status and recent matches.

use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Shape, Stroke, Ui,
};

use super::icons::{self, Icon};
use super::theme::{self, pal, Palette};
use super::widgets;
use crate::app::{AppState, Screen};
use crate::content::valorant_api::queue_name;
use crate::content::Catalog;
use crate::ipc::{CareerSummary, ClientStatus, IntentTx};
use crate::riot::models::{MatchDetail, Outcome};
use crate::riot::presence::LoopState;
use crate::stats::{self, ActivityKind, Overview, Range};

const GAP: f32 = 16.0;
/// Below this content width the right column moves under the left one.
const TWO_COLUMN_MIN: f32 = 900.0;
/// Height of the Performance / Top Agents row.
const ROW2_H: f32 = 320.0;
/// Height of the Rank Progression / Recent Activity row.
const ROW3_H: f32 = 250.0;

pub fn ui(ui: &mut Ui, state: &mut AppState, intents: &IntentTx) {
    let Some(career) = state.career.clone() else {
        theme::card(ui, Some(160.0), |ui| {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                if matches!(state.status, ClientStatus::Authed { .. }) {
                    ui.spinner();
                    ui.label("Loading your profile…");
                } else {
                    ui.label(RichText::new("Start VALORANT to see your stats").size(16.0));
                    ui.weak("Your rank, matches and lobbies show up here once the Riot Client is connected.");
                }
            });
        });
        return;
    };
    let now = chrono::Utc::now().timestamp();
    let owned: Vec<std::sync::Arc<MatchDetail>> = career
        .matches
        .iter()
        .filter_map(|e| state.matches.get(&e.match_id).cloned())
        .collect();
    let refs: Vec<&MatchDetail> = owned.iter().map(|m| m.as_ref()).collect();
    let overview = stats::overview(&career.puuid, &refs);

    let total = ui.available_width();
    let two_col = total >= TWO_COLUMN_MIN;
    let right_w = if two_col {
        (total * 0.27).clamp(300.0, 380.0)
    } else {
        total
    };
    let left_w = if two_col { total - right_w - GAP } else { total };

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        column(ui, left_w, |ui| {
            left_column(ui, state, intents, &career, &refs, &overview, now)
        });
        if two_col {
            column(ui, right_w, |ui| right_column(ui, state, intents, &career, &refs, now));
        }
    });
    if !two_col {
        ui.add_space(GAP);
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        right_column(ui, state, intents, &career, &refs, now);
    }
}

fn column(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        add(ui)
    });
}

/// Two side-by-side cells of equal height (`ratio` = left share).
fn row(ui: &mut Ui, ratio: f32, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    let w = ui.available_width();
    let lw = ((w - GAP) * ratio).floor();
    let rw = w - GAP - lw;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        column(ui, lw, left);
        column(ui, rw, right);
    });
}

// ------------------------------------------------------------------ left --

fn left_column(
    ui: &mut Ui,
    state: &mut AppState,
    intents: &IntentTx,
    career: &CareerSummary,
    refs: &[&MatchDetail],
    o: &Overview,
    now: i64,
) {
    let cat = state.catalog.clone();
    banner(ui, career, &cat);
    stat_cards(ui, career, o);
    let mut range = state.home_range;
    let mut agents_clicked = false;
    row(
        ui,
        0.58,
        |ui| performance(ui, &mut range, &career.puuid, refs, now),
        |ui| agents_clicked = top_agents(ui, &cat, &career.puuid, refs),
    );
    state.home_range = range;
    if agents_clicked {
        state.screen = Screen::Agents;
    }
    row(
        ui,
        0.58,
        |ui| rank_progression(ui, &cat, career),
        |ui| recent_activity(ui, &cat, career, now),
    );
    theme::card(ui, None, |ui| {
        if theme::card_title(ui, "Match History", Some("View all")) {
            state.screen = Screen::Matches;
        }
        let entries: Vec<_> = career.matches.iter().take(5).cloned().collect();
        if let Some(id) = super::career::match_table(ui, state, &career.puuid, &entries, "home-history") {
            state.open_match(id, None, intents);
        }
    });
}

fn banner(ui: &mut Ui, career: &CareerSummary, cat: &Catalog) {
    let p = pal(ui.ctx());
    let h = 150.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let r = CornerRadius::same(14);
    ui.painter().rect(
        rect,
        r,
        p.card,
        Stroke::new(1.0_f32, p.stroke),
        egui::StrokeKind::Inside,
    );

    // Right: the player's card art, fading into the card.
    let art = Rect::from_min_max(pos2(rect.left() + rect.width() * 0.52, rect.top()), rect.max);
    match &career.profile.card_wide {
        Some(uri) => {
            let cr = CornerRadius {
                nw: 0,
                sw: 0,
                ne: 14,
                se: 14,
            };
            theme::image_cover(ui, art, Some(uri), 452.0 / 128.0, cr);
        }
        None => shards(ui.painter(), art, &p),
    }
    fade(
        ui.painter(),
        Rect::from_min_size(art.min, vec2(110.0, art.height())),
        p.card,
    );

    // Left: avatar, name, rank.
    let inner = Rect::from_min_max(rect.min + vec2(26.0, 0.0), pos2(art.left(), rect.bottom()));
    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 2.0);
            let initials: String = career
                .riot_id
                .as_ref()
                .map(|i| i.game_name.chars().take(2).collect())
                .unwrap_or_default();
            theme::avatar(ui, career.profile.card_icon.as_deref(), &initials.to_uppercase(), 92.0);
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let (name, tag) = career
                        .riot_id
                        .as_ref()
                        .map(|i| (i.game_name.clone(), format!("#{}", i.tag_line)))
                        .unwrap_or_else(|| ("You".into(), String::new()));
                    ui.label(RichText::new(name).size(24.0).color(p.text));
                    ui.label(RichText::new(tag).size(12.0).color(p.faint));
                });
                let mut chips = Vec::new();
                if let Some(t) = &career.profile.title {
                    chips.push(t.clone());
                }
                if let Some(l) = career.profile.account_level {
                    chips.push(format!("Level {l}"));
                }
                if !chips.is_empty() {
                    ui.label(RichText::new(chips.join(" · ")).size(11.5).color(p.weak));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    widgets::rank_icon(ui, cat, career.rank.tier, 34.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(cat.tier_name(career.rank.tier)).size(14.5).color(p.text));
                        if career.rank.tier > 0 {
                            ui.label(RichText::new(format!("{} RR", career.rank.rr)).size(11.5).color(p.weak));
                        }
                    });
                });
            });
        },
    );
}

/// Original decorative shapes when there's no card art.
fn shards(painter: &egui::Painter, r: Rect, p: &Palette) {
    let at = |x: f32, y: f32| pos2(r.left() + r.width() * x, r.top() + r.height() * y);
    let fill = |a: f32| p.accent.gamma_multiply(a);
    painter.rect_filled(
        r,
        CornerRadius {
            nw: 0,
            sw: 0,
            ne: 14,
            se: 14,
        },
        p.accent.gamma_multiply(0.06),
    );
    for (pts, a) in [
        (vec![at(0.30, 0.0), at(0.62, 0.0), at(0.44, 0.75)], 0.22),
        (vec![at(0.55, 0.25), at(0.95, 0.10), at(0.80, 1.0)], 0.35),
        (vec![at(0.70, 0.0), at(1.0, 0.0), at(1.0, 0.45)], 0.18),
        (vec![at(0.20, 1.0), at(0.48, 0.55), at(0.60, 1.0)], 0.14),
    ] {
        painter.add(Shape::convex_polygon(pts, fill(a), Stroke::NONE));
    }
}

/// Horizontal gradient from `color` (left) to transparent (right).
fn fade(painter: &egui::Painter, r: Rect, color: Color32) {
    let mut mesh = egui::Mesh::default();
    let clear = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 0);
    mesh.colored_vertex(r.left_top(), color);
    mesh.colored_vertex(r.right_top(), clear);
    mesh.colored_vertex(r.right_bottom(), clear);
    mesh.colored_vertex(r.left_bottom(), color);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

fn stat_cards(ui: &mut Ui, career: &CareerSummary, o: &Overview) {
    let w = ui.available_width();
    let cw = ((w - 3.0 * GAP) / 4.0).floor();
    let recent_wr = o.me.win_rate();
    let act_wr = career.rank.win_rate();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        column(ui, cw, |ui| {
            let (value, sub) = match act_wr {
                Some(wr) => (
                    wr,
                    format!(
                        "{}W / {}L this act",
                        career.rank.season_wins,
                        career.rank.season_games - career.rank.season_wins
                    ),
                ),
                None => (
                    recent_wr.unwrap_or(0.0),
                    format!("{}W / {}L recent", o.me.wins, o.me.losses),
                ),
            };
            stat_card(
                ui,
                "Win Rate",
                &format!("{value:.1}%"),
                &sub,
                None,
                Some(value / 100.0),
                |ui| {
                    if let (Some(r), Some(a)) = (recent_wr, act_wr) {
                        theme::delta_with_unit(ui, r - a, " pts", &format!("last {} games", o.me.matches));
                    }
                },
            );
        });
        let lobby = |ui: &mut Ui, me: f32, avg: f32| {
            if let Some(d) = stats::relative_delta(me, avg) {
                theme::delta(ui, d, "vs lobby");
            }
        };
        let has = o.me.matches > 0;
        column(ui, cw, |ui| {
            let sub = if has {
                format!("Lobby avg {:.2}", o.lobby.kd())
            } else {
                "No matches yet".into()
            };
            stat_card(
                ui,
                "K/D Ratio",
                &if has { format!("{:.2}", o.me.kd()) } else { "–".into() },
                &sub,
                Some(Icon::Swords),
                None,
                |ui| {
                    if has {
                        lobby(ui, o.me.kd(), o.lobby.kd())
                    }
                },
            );
        });
        column(ui, cw, |ui| {
            let sub = if has {
                format!("Lobby avg {:.0}", o.lobby.acs())
            } else {
                "No matches yet".into()
            };
            stat_card(
                ui,
                "ACS",
                &if has {
                    format!("{:.0}", o.me.acs())
                } else {
                    "–".into()
                },
                &sub,
                Some(Icon::Crosshair),
                None,
                |ui| {
                    if has {
                        lobby(ui, o.me.acs(), o.lobby.acs())
                    }
                },
            );
        });
        column(ui, cw, |ui| {
            let sub = if has {
                format!("Lobby avg {:.1}%", o.lobby.hs_pct())
            } else {
                "No matches yet".into()
            };
            stat_card(
                ui,
                "Headshot %",
                &if has {
                    format!("{:.1}%", o.me.hs_pct())
                } else {
                    "–".into()
                },
                &sub,
                Some(Icon::Target),
                None,
                |ui| {
                    if has {
                        lobby(ui, o.me.hs_pct(), o.lobby.hs_pct())
                    }
                },
            );
        });
    });
}

fn stat_card(
    ui: &mut Ui,
    label: &str,
    value: &str,
    sub: &str,
    icon: Option<Icon>,
    ring: Option<f32>,
    footer: impl FnOnce(&mut Ui),
) {
    let p = pal(ui.ctx());
    theme::card(ui, Some(118.0), |ui| {
        let top = ui.max_rect().right_top();
        ui.label(RichText::new(label).size(12.0).color(p.weak));
        ui.label(RichText::new(value).size(26.0).color(p.text));
        ui.label(RichText::new(sub).size(11.5).color(p.weak));
        footer(ui);
        let badge = Rect::from_center_size(top + vec2(-16.0, 18.0), vec2(30.0, 30.0));
        if let Some(frac) = ring {
            progress_ring(ui.painter(), badge.center(), 15.0, frac, &p);
        } else if let Some(i) = icon {
            ui.painter().circle_filled(badge.center(), 15.0, p.well);
            icons::paint(ui.painter(), badge.shrink(8.0), p.weak, i);
        }
    });
}

fn progress_ring(painter: &egui::Painter, c: egui::Pos2, r: f32, frac: f32, p: &Palette) {
    painter.circle_stroke(c, r, Stroke::new(3.0_f32, p.well));
    let n = 48;
    let end = frac.clamp(0.0, 1.0) * std::f32::consts::TAU;
    let pts: Vec<_> = (0..=n)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + end * i as f32 / n as f32;
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    painter.add(Shape::line(pts, Stroke::new(3.0_f32, p.win)));
}

fn performance(ui: &mut Ui, range: &mut Range, puuid: &str, refs: &[&MatchDetail], now: i64) {
    let p = pal(ui.ctx());
    theme::card(ui, Some(ROW2_H), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Performance").size(15.0).strong().color(p.text));
            ui.label(RichText::new("ACS per match").size(11.0).color(p.faint));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for r in Range::ALL.iter().rev() {
                    if theme::pill(ui, r.label(), *range == *r).clicked() {
                        *range = *r;
                    }
                }
            });
        });
        ui.add_space(8.0);
        let series = stats::acs_series(puuid, refs, *range, now);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW2_H - 92.0), Sense::hover());
        line_chart(ui, rect, &series, &p, resp.hover_pos());
    });
}

/// Line chart with gridlines, value labels, date labels and a soft fill.
fn line_chart(ui: &Ui, rect: Rect, series: &[(i64, f32)], p: &Palette, hover: Option<egui::Pos2>) {
    let painter = ui.painter_at(rect);
    if series.len() < 2 {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Not enough matches in this range",
            FontId::proportional(12.0),
            p.weak,
        );
        return;
    }
    let (lo, hi) = series
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), (_, v)| (a.min(*v), b.max(*v)));
    let step = 50.0;
    let lo = ((lo / step).floor() * step - step).max(0.0);
    let hi = (hi / step).ceil() * step + step * 0.0 + step;
    let plot = Rect::from_min_max(rect.min + vec2(34.0, 6.0), rect.max - vec2(8.0, 22.0));
    let y_of = |v: f32| plot.bottom() - (v - lo) / (hi - lo) * plot.height();
    let x_of = |i: usize| plot.left() + plot.width() * i as f32 / (series.len() - 1) as f32;

    let lines = 4;
    for k in 0..=lines {
        let v = lo + (hi - lo) * k as f32 / lines as f32;
        let y = y_of(v);
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0_f32, p.grid),
        );
        painter.text(
            pos2(rect.left(), y),
            Align2::LEFT_CENTER,
            format!("{v:.0}"),
            FontId::proportional(10.5),
            p.weak,
        );
    }
    let pts: Vec<_> = series
        .iter()
        .enumerate()
        .map(|(i, (_, v))| pos2(x_of(i), y_of(*v)))
        .collect();

    // Fill under the line.
    let mut mesh = egui::Mesh::default();
    let top = p.navy.gamma_multiply(0.14);
    let bottom = p.navy.gamma_multiply(0.0);
    for (i, pt) in pts.iter().enumerate() {
        mesh.colored_vertex(*pt, top);
        mesh.colored_vertex(pos2(pt.x, plot.bottom()), bottom);
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b + 1, b);
        }
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::line(pts.clone(), Stroke::new(1.8_f32, p.navy)));
    for pt in &pts {
        painter.circle_filled(*pt, 2.6, p.navy);
    }

    // Up to 7 date labels.
    let n = series.len();
    let labels = n.min(7);
    for k in 0..labels {
        let i = if labels == 1 { 0 } else { k * (n - 1) / (labels - 1) };
        let date = chrono::DateTime::from_timestamp(series[i].0, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%b %d").to_string())
            .unwrap_or_default();
        painter.text(
            pos2(x_of(i), rect.bottom() - 6.0),
            Align2::CENTER_CENTER,
            date,
            FontId::proportional(10.5),
            p.weak,
        );
    }

    // Hover: nearest point's value.
    if let Some(h) = hover {
        if let Some((i, pt)) = pts
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1.x - h.x).abs().total_cmp(&(b.1.x - h.x).abs()))
        {
            painter.circle_stroke(*pt, 5.0, Stroke::new(1.5_f32, p.accent));
            let label = format!("{:.0} ACS", series[i].1);
            painter.text(
                *pt - vec2(0.0, 12.0),
                Align2::CENTER_BOTTOM,
                label,
                FontId::proportional(11.5),
                p.text,
            );
        }
    }
}

/// Returns true when "View all" was clicked.
fn top_agents(ui: &mut Ui, cat: &Catalog, puuid: &str, refs: &[&MatchDetail]) -> bool {
    let p = pal(ui.ctx());
    theme::card(ui, Some(ROW2_H), |ui| {
        let clicked = theme::card_title(ui, "Top Agents", Some("View all"));
        let agents = stats::agents(puuid, refs);
        if agents.is_empty() {
            ui.weak("Play a few matches to see your agents.");
        }
        let most = agents.first().map(|a| a.totals.matches).unwrap_or(1).max(1);
        for a in agents.iter().take(5) {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                theme::image_cover(ui, r, cat.agent(&a.agent_id).map(|x| x.icon.as_str()), 1.0, 17);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let name = cat
                        .agent(&a.agent_id)
                        .map(|x| x.name.clone())
                        .unwrap_or_else(|| "Unknown".into());
                    ui.label(RichText::new(name).size(13.0).color(p.text));
                    let wr = a
                        .totals
                        .win_rate()
                        .map(|w| format!(" · {w:.0}% WR"))
                        .unwrap_or_default();
                    ui.label(
                        RichText::new(format!("{:.2} K/D{wr} · {} games", a.totals.kd(), a.totals.matches))
                            .size(11.0)
                            .color(p.weak),
                    );
                    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
                    ui.painter().rect_filled(bar, 2.0, p.well);
                    let filled = Rect::from_min_size(
                        bar.min,
                        vec2(bar.width() * a.totals.matches as f32 / most as f32, bar.height()),
                    );
                    ui.painter().rect_filled(filled, 2.0, p.navy.gamma_multiply(0.7));
                });
            });
            ui.add_space(4.0);
        }
        clicked
    })
}

fn rank_progression(ui: &mut Ui, cat: &Catalog, career: &CareerSummary) {
    let p = pal(ui.ctx());
    theme::card(ui, Some(ROW3_H), |ui| {
        theme::card_title(ui, "Rank Progression", None);
        let pts = stats::rank_milestones(&career.rr_history, 5);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW3_H - 76.0), Sense::hover());
        let painter = ui.painter_at(rect);
        if pts.len() < 2 {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Play ranked to see your progression",
                FontId::proportional(12.0),
                p.weak,
            );
            return;
        }
        let ladder: Vec<f32> = pts.iter().map(|x| x.tier as f32 * 100.0 + x.rr as f32).collect();
        let (lo, hi) = ladder
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        let plot = Rect::from_min_max(rect.min + vec2(28.0, 30.0), rect.max - vec2(28.0, 42.0));
        let y_of = |v: f32| {
            if hi > lo {
                plot.bottom() - (v - lo) / (hi - lo) * plot.height()
            } else {
                plot.center().y
            }
        };
        let xy: Vec<_> = ladder
            .iter()
            .enumerate()
            .map(|(i, v)| {
                pos2(
                    plot.left() + plot.width() * i as f32 / (ladder.len() - 1) as f32,
                    y_of(*v),
                )
            })
            .collect();
        let mut mesh = egui::Mesh::default();
        for (i, pt) in xy.iter().enumerate() {
            mesh.colored_vertex(*pt, p.win.gamma_multiply(0.22));
            mesh.colored_vertex(pos2(pt.x, plot.bottom() + 8.0), p.win.gamma_multiply(0.0));
            if i > 0 {
                let b = (i as u32) * 2;
                mesh.add_triangle(b - 2, b - 1, b);
                mesh.add_triangle(b - 1, b + 1, b);
            }
        }
        painter.add(Shape::mesh(mesh));
        painter.add(Shape::line(xy.clone(), Stroke::new(1.8_f32, p.win)));
        for (pt, m) in xy.iter().zip(&pts) {
            let icon = Rect::from_center_size(*pt - vec2(0.0, 2.0), vec2(26.0, 26.0));
            match cat.tier(m.tier).map(|t| t.icon.clone()).filter(|u| !u.is_empty()) {
                Some(u) => egui::Image::new(u).paint_at(ui, icon),
                None => {
                    painter.circle_filled(*pt, 5.0, p.card);
                    painter.circle_stroke(*pt, 5.0, Stroke::new(2.0_f32, p.win));
                }
            }
            let date = chrono::DateTime::from_timestamp(m.at_unix, 0)
                .map(|d| d.with_timezone(&chrono::Local).format("%b %d").to_string())
                .unwrap_or_default();
            painter.text(
                pos2(pt.x, rect.bottom() - 24.0),
                Align2::CENTER_CENTER,
                cat.tier_name(m.tier),
                FontId::proportional(11.0),
                p.text,
            );
            painter.text(
                pos2(pt.x, rect.bottom() - 10.0),
                Align2::CENTER_CENTER,
                date,
                FontId::proportional(10.0),
                p.weak,
            );
        }
    });
}

fn recent_activity(ui: &mut Ui, cat: &Catalog, career: &CareerSummary, now: i64) {
    let p = pal(ui.ctx());
    theme::card(ui, Some(ROW3_H), |ui| {
        theme::card_title(ui, "Recent Activity", None);
        let items = stats::activity(&career.rr_history, 4);
        if items.is_empty() {
            ui.weak("Ranked games, promotions and demotions show up here.");
        }
        for a in items {
            let (icon, color, title) = match a.kind {
                ActivityKind::RankUp => (Icon::ChartUp, p.win, format!("Promoted to {}", cat.tier_name(a.tier))),
                ActivityKind::RankDown => (Icon::ChartDown, p.loss, format!("Dropped to {}", cat.tier_name(a.tier))),
                ActivityKind::Match => (Icon::Gamepad, p.weak, "Completed match".to_string()),
            };
            let sign = if a.rr_earned >= 0 { "+" } else { "" };
            let sub = format!("Competitive · {} · {sign}{} RR", cat.map_name(&a.map_id), a.rr_earned);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 15.0, color.gamma_multiply(0.14));
                icons::paint(ui.painter(), r.shrink(8.0), color, icon);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(title).size(12.5).color(p.text));
                    ui.label(RichText::new(sub).size(11.0).color(p.weak));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(stats::ago(a.at_unix, now)).size(11.0).color(p.faint));
                });
            });
            ui.add_space(2.0);
        }
    });
}

// ----------------------------------------------------------------- right --

fn right_column(
    ui: &mut Ui,
    state: &mut AppState,
    intents: &IntentTx,
    career: &CareerSummary,
    refs: &[&MatchDetail],
    now: i64,
) {
    let p = pal(ui.ctx());
    let cat = state.catalog.clone();
    theme::card(ui, None, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Current Act").size(15.0).strong().color(p.text));
            if let Some(name) = &cat.current_act_name {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(name).size(11.0).color(p.faint));
                });
            }
        });
        ui.add_space(10.0);
        let (title, sub) = status_text(state);
        ui.horizontal(|ui| {
            widgets::rank_icon(ui, &cat, career.rank.tier, 40.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(title).size(15.0).color(p.text));
                ui.label(RichText::new(sub).size(11.5).color(p.weak));
            });
        });
        ui.add_space(12.0);
        if theme::primary_button(ui, "Open live view").clicked() {
            state.screen = Screen::Live;
        }
    });

    theme::card(ui, None, |ui| {
        if theme::card_title(ui, "Recent Matches", Some("View all")) {
            state.screen = Screen::Matches;
        }
        if refs.is_empty() {
            ui.weak("Loading matches…");
        }
        let mut open = None;
        for m in refs.iter().take(6) {
            if recent_match_row(ui, &cat, &career.puuid, m, now).clicked() {
                open = Some(m.info.match_id.clone());
            }
        }
        if let Some(id) = open {
            state.open_match(id, None, intents);
        }
    });
}

fn status_text(state: &AppState) -> (String, String) {
    if !matches!(state.status, ClientStatus::Authed { .. }) {
        return ("Offline".into(), "Start VALORANT to see your lobby".into());
    }
    let Some(g) = &state.game else {
        return ("Online".into(), "Waiting for game state…".into());
    };
    let queue = if g.queue_id.is_empty() {
        String::new()
    } else {
        queue_name(&g.queue_id).to_string()
    };
    let map = if g.map.is_empty() {
        String::new()
    } else {
        state.catalog.map_name(&g.map)
    };
    let sub = [queue, map]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let title = if g.in_queue() {
        "In Queue"
    } else {
        match g.loop_state {
            LoopState::Pregame => "Agent Select",
            LoopState::InGame => "In Game",
            LoopState::Menus => "In Menus",
            LoopState::Unknown => "Online",
        }
    };
    let sub = if sub.is_empty() && g.party_size > 0 {
        format!("Party of {}", g.party_size)
    } else {
        sub
    };
    (title.into(), sub)
}

fn recent_match_row(ui: &mut Ui, cat: &Catalog, puuid: &str, m: &MatchDetail, now: i64) -> egui::Response {
    let p = pal(ui.ctx());
    let h = 74.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect.expand(4.0), 10.0, p.well.gamma_multiply(0.6));
    }
    let img = Rect::from_min_size(rect.min + vec2(0.0, 5.0), vec2(64.0, 64.0));
    theme::image_cover(
        ui,
        img,
        cat.map(&m.info.map_id).map(|x| x.splash.as_str()),
        16.0 / 9.0,
        10,
    );
    let x = img.right() + 14.0;
    let painter = ui.painter();
    let me = m.player(puuid);
    let (label, color, a, b) = match m.outcome_for(puuid) {
        Some((Outcome::Win, a, b)) => ("Victory", p.win, a, b),
        Some((Outcome::Loss, a, b)) => ("Defeat", p.loss, a, b),
        Some((Outcome::Draw, a, b)) => ("Draw", p.weak, a, b),
        None => ("—", p.weak, 0, 0),
    };
    let f = |s: f32| FontId::proportional(s);
    painter.text(
        pos2(x, rect.top() + 8.0),
        Align2::LEFT_TOP,
        cat.map_name(&m.info.map_id),
        f(12.0),
        p.text,
    );
    painter.text(pos2(x, rect.top() + 23.0), Align2::LEFT_TOP, label, f(14.5), color);
    painter.text(
        pos2(x, rect.top() + 42.0),
        Align2::LEFT_TOP,
        format!("{a} - {b}"),
        f(12.0),
        color,
    );
    if let Some(me) = me {
        let s = &me.stats;
        painter.text(
            pos2(x, rect.top() + 57.0),
            Align2::LEFT_TOP,
            format!("{} / {} / {}", s.kills, s.deaths, s.assists),
            f(11.5),
            p.text,
        );
        let mid = rect.left() + rect.width() * 0.66;
        painter.text(pos2(mid, rect.top() + 40.0), Align2::LEFT_TOP, "ACS", f(10.0), p.faint);
        painter.text(
            pos2(mid, rect.top() + 53.0),
            Align2::LEFT_TOP,
            format!("{:.0}", s.acs()),
            f(12.5),
            p.text,
        );
    }
    painter.text(
        rect.right_center() - vec2(2.0, 0.0),
        Align2::RIGHT_CENTER,
        stats::ago(m.info.started_at_unix, now),
        f(11.0),
        p.faint,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
