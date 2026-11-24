//! Building blocks shared by the stats screens (Home, Recaps, Mastery and
//! other players' profiles): stat tiles, the performance chart, agent and
//! map lists, performance and accolade badges.

use std::sync::Arc;

use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Shape, Stroke, Ui};

use super::icons::{self, Icon};
use super::theme::{self, pal, Palette};
use crate::app::AppState;
use crate::content::Catalog;
use crate::insights::{Accolade, MapStats, Rarity};
use crate::riot::models::MatchDetail;
use crate::stats::{AgentStats, Overview, Totals};

/// Every loaded, finished match `puuid` played in, newest first.
pub fn matches_of(state: &AppState, puuid: &str) -> Vec<Arc<MatchDetail>> {
    let mut v: Vec<Arc<MatchDetail>> = state
        .matches
        .values()
        .filter(|m| m.player(puuid).is_some())
        .cloned()
        .collect();
    v.sort_by_key(|m| std::cmp::Reverse(m.info.started_at_unix));
    v
}

/// Colour for a performance score (0–500): gold from 325, green from 275, red at 200 or below.
pub fn perf_color(perf: f32, p: &Palette) -> Color32 {
    if perf >= 325.0 {
        p.gold
    } else if perf >= 275.0 {
        p.win
    } else if perf <= 200.0 {
        p.loss
    } else {
        p.text
    }
}

/// A word for a performance score ("Elite", "Great"…).
pub fn perf_label(perf: f32) -> &'static str {
    match perf as u32 {
        350.. => "Elite",
        300..=349 => "Great",
        260..=299 => "Good",
        230..=259 => "Average",
        190..=229 => "Below average",
        _ => "Rough",
    }
}

/// A rounded badge with a performance score in it.
pub fn perf_badge(painter: &egui::Painter, rect: Rect, perf: Option<u32>, p: &Palette) {
    let (text, c) = match perf {
        Some(v) => (v.to_string(), perf_color(v as f32, p)),
        None => ("—".into(), p.weak),
    };
    painter.rect_filled(rect, CornerRadius::same(8), c.gamma_multiply(0.14));
    painter.rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0_f32, c.gamma_multiply(0.35)),
        egui::StrokeKind::Inside,
    );
    painter.text(rect.center(), Align2::CENTER_CENTER, text, theme::semibold(13.0), c);
}

/// One headline stat tile.
pub struct Tile {
    pub label: &'static str,
    pub value: String,
    pub sub: String,
    pub icon: Icon,
    pub tint: Color32,
    /// Relative difference vs. the lobby, in % (or points when `points`).
    pub delta: Option<f32>,
    pub points: bool,
}

/// A wrapping row of stat tiles.
pub fn tiles(ui: &mut Ui, tiles: &[Tile], min_w: f32) {
    theme::tiles(ui, tiles, min_w, 14.0, |ui, t| {
        let p = pal(ui.ctx());
        theme::card(ui, Some(112.0), |ui| {
            let top = ui.max_rect().right_top();
            ui.label(RichText::new(t.label).size(12.0).color(p.weak));
            ui.label(RichText::new(&t.value).font(theme::semibold(25.0)).color(p.text));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(d) = t.delta {
                    if t.points {
                        theme::delta_with_unit(ui, d, " pts", "");
                    } else {
                        theme::delta(ui, d, "");
                    }
                }
                ui.label(RichText::new(&t.sub).size(11.0).color(p.faint));
            });
            let badge = Rect::from_center_size(top + vec2(-16.0, 16.0), vec2(32.0, 32.0));
            theme::icon_badge(ui.painter(), badge, t.icon, t.tint);
        });
    });
}

/// The standard six tiles for a set of matches compared with the lobbies.
pub fn overview_tiles(o: &Overview, p: &Palette) -> Vec<Tile> {
    let has = o.me.matches > 0;
    let dash = || "–".to_string();
    let rel = |me: f32, lobby: f32| crate::stats::relative_delta(me, lobby);
    let lobby_perf = o.lobby.perf().unwrap_or(crate::stats::PERF_AVG);
    vec![
        Tile {
            label: "Performance",
            value: o.me.perf().map(|v| format!("{v:.0}")).unwrap_or_else(dash),
            sub: o
                .me
                .perf()
                .map(|v| perf_label(v).to_string())
                .unwrap_or_else(|| "No matches yet".into()),
            icon: Icon::Lightning,
            tint: p.accent,
            delta: o.me.perf().map(|v| v - lobby_perf),
            points: true,
        },
        Tile {
            label: "Win rate",
            value: o.me.win_rate().map(|v| format!("{v:.0}%")).unwrap_or_else(dash),
            sub: format!("{}W {}L", o.me.wins, o.me.losses),
            icon: Icon::Trophy,
            tint: p.win,
            delta: None,
            points: false,
        },
        Tile {
            label: "K/D",
            value: if has { format!("{:.2}", o.me.kd()) } else { dash() },
            sub: if has {
                format!("KDA {:.2}", o.me.kda())
            } else {
                String::new()
            },
            icon: Icon::Swords,
            tint: p.accent2,
            delta: if has { rel(o.me.kd(), o.lobby.kd()) } else { None },
            points: false,
        },
        Tile {
            label: "Headshot %",
            value: if has { format!("{:.1}%", o.me.hs_pct()) } else { dash() },
            sub: "vs lobby".into(),
            icon: Icon::Target,
            tint: p.gold,
            delta: if has {
                rel(o.me.hs_pct(), o.lobby.hs_pct())
            } else {
                None
            },
            points: false,
        },
        Tile {
            label: "Damage / round",
            value: if has { format!("{:.0}", o.me.adr()) } else { dash() },
            sub: "vs lobby".into(),
            icon: Icon::Fire,
            tint: p.warn,
            delta: if has { rel(o.me.adr(), o.lobby.adr()) } else { None },
            points: false,
        },
        Tile {
            label: "KAST",
            value: if has {
                format!("{:.0}%", o.me.kast_pct())
            } else {
                dash()
            },
            sub: "vs lobby".into(),
            icon: Icon::Shield,
            tint: p.win,
            delta: if has {
                rel(o.me.kast_pct(), o.lobby.kast_pct())
            } else {
                None
            },
            points: false,
        },
    ]
}

/// Area chart of performance scores with the 250 "lobby average" line.
pub fn perf_chart(ui: &mut Ui, height: f32, series: &[(i64, f32)], intro_key: u64) {
    let p = pal(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    if series.len() < 2 {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Play a couple of matches to see a trend",
            FontId::proportional(12.0),
            p.weak,
        );
        return;
    }
    let t = theme::intro(ui.ctx(), resp.id.with("intro"), intro_key, 0.6);
    let (lo, hi) = series
        .iter()
        .fold((crate::stats::PERF_AVG, crate::stats::PERF_AVG), |(a, b), (_, v)| {
            (a.min(*v), b.max(*v))
        });
    let lo = ((lo - 30.0) / 50.0).floor().max(0.0) * 50.0;
    let hi = ((hi + 30.0) / 50.0).ceil().min(10.0) * 50.0;
    let plot = Rect::from_min_max(rect.min + vec2(34.0, 8.0), rect.max - vec2(6.0, 20.0));
    let y_of = |v: f32| plot.bottom() - (v - lo) / (hi - lo).max(1.0) * plot.height();
    let x_of = |i: usize| plot.left() + plot.width() * i as f32 / (series.len() - 1) as f32;
    let mut v = lo;
    while v <= hi + 0.5 {
        let y = y_of(v);
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0_f32, p.grid),
        );
        painter.text(
            pos2(rect.left(), y),
            Align2::LEFT_CENTER,
            format!("{v:.0}"),
            FontId::proportional(10.0),
            p.faint,
        );
        v += 50.0;
    }
    // Lobby average.
    let avg_y = y_of(crate::stats::PERF_AVG);
    let mut x = plot.left();
    while x < plot.right() {
        painter.line_segment(
            [pos2(x, avg_y), pos2((x + 5.0).min(plot.right()), avg_y)],
            Stroke::new(1.0_f32, p.weak.gamma_multiply(0.6)),
        );
        x += 9.0;
    }
    painter.text(
        pos2(plot.right(), avg_y - 3.0),
        Align2::RIGHT_BOTTOM,
        "lobby avg",
        FontId::proportional(9.5),
        p.faint,
    );
    let pts: Vec<_> = series
        .iter()
        .enumerate()
        .map(|(i, (_, v))| pos2(x_of(i), plot.bottom() - (plot.bottom() - y_of(*v)) * t))
        .collect();
    let mut mesh = egui::Mesh::default();
    let top = p.accent.gamma_multiply(if p.dark { 0.28 } else { 0.18 });
    for (i, pt) in pts.iter().enumerate() {
        mesh.colored_vertex(*pt, top);
        mesh.colored_vertex(pos2(pt.x, plot.bottom()), Color32::TRANSPARENT);
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b + 1, b);
        }
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::line(pts.clone(), Stroke::new(2.2_f32, p.accent)));
    for (pt, (_, v)) in pts.iter().zip(series) {
        painter.circle_filled(*pt, 3.2, p.card);
        painter.circle_stroke(*pt, 3.2, Stroke::new(1.6_f32, perf_color(*v, &p)));
    }
    let n = series.len();
    let labels = n.min(6);
    for k in 0..labels {
        let i = if labels == 1 { 0 } else { k * (n - 1) / (labels - 1) };
        let date = chrono::DateTime::from_timestamp(series[i].0, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%b %d").to_string())
            .unwrap_or_default();
        painter.text(
            pos2(x_of(i), rect.bottom() - 6.0),
            Align2::CENTER_CENTER,
            date,
            FontId::proportional(10.0),
            p.faint,
        );
    }
    if let Some(h) = resp.hover_pos() {
        if let Some((i, pt)) = pts
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1.x - h.x).abs().total_cmp(&(b.1.x - h.x).abs()))
        {
            painter.line_segment(
                [pos2(pt.x, plot.top()), pos2(pt.x, plot.bottom())],
                Stroke::new(1.0_f32, p.stroke),
            );
            painter.circle_stroke(*pt, 6.0, Stroke::new(1.5_f32, p.accent));
            let label = format!("{:.0} · {}", series[i].1, perf_label(series[i].1));
            let galley = painter.layout_no_wrap(label, theme::medium(11.5), p.text);
            let at = (*pt - vec2(galley.size().x / 2.0, galley.size().y + 16.0)).max(rect.min);
            let bg = Rect::from_min_size(at - vec2(8.0, 4.0), galley.size() + vec2(16.0, 8.0));
            painter.rect_filled(bg, CornerRadius::same(8), p.selected);
            painter.galley(at, galley, p.text);
        }
    }
}

/// Agent rows: icon, name, games, win rate, K/D and performance.
pub fn agent_rows(ui: &mut Ui, cat: &Catalog, agents: &[AgentStats], max: usize) {
    let p = pal(ui.ctx());
    if agents.is_empty() {
        ui.label(RichText::new("No agent games loaded yet.").color(p.weak));
        return;
    }
    let most = agents.iter().map(|a| a.totals.matches).max().unwrap_or(1).max(1);
    for a in agents.iter().take(max) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
        let icon = Rect::from_min_size(rect.min + vec2(0.0, 5.0), vec2(36.0, 36.0));
        theme::image_cover(ui, icon, cat.agent(&a.agent_id).map(|x| x.icon.as_str()), 1.0, 10);
        if cat.agent(&a.agent_id).is_none_or(|x| x.icon.is_empty()) {
            initials(ui.painter(), icon, &agent_name(cat, &a.agent_id), &p);
        }
        let painter = ui.painter();
        let x = icon.right() + 12.0;
        painter.text(
            pos2(x, rect.top() + 8.0),
            Align2::LEFT_TOP,
            agent_name(cat, &a.agent_id),
            theme::semibold(13.0),
            p.text,
        );
        let t = &a.totals;
        let wr = t.win_rate().map(|w| format!("{w:.0}% WR")).unwrap_or_default();
        painter.text(
            pos2(x, rect.top() + 26.0),
            Align2::LEFT_TOP,
            format!("{} · {wr} · {:.2} K/D", games(t.matches), t.kd()),
            FontId::proportional(11.0),
            p.weak,
        );
        let bar_w = (rect.width() * 0.22).clamp(50.0, 140.0);
        let bar = Rect::from_min_size(
            pos2(rect.right() - 58.0 - bar_w, rect.center().y - 2.0),
            vec2(bar_w, 4.0),
        );
        painter.rect_filled(bar, 2.0, p.well);
        painter.rect_filled(
            Rect::from_min_size(bar.min, vec2(bar.width() * t.matches as f32 / most as f32, 4.0)),
            2.0,
            p.accent2,
        );
        perf_badge(
            painter,
            Rect::from_min_size(pos2(rect.right() - 46.0, rect.center().y - 13.0), vec2(46.0, 26.0)),
            t.perf().map(|v| v.round() as u32),
            &p,
        );
    }
}

/// Map rows: name over the splash art, games, win rate bar.
pub fn map_rows(ui: &mut Ui, cat: &Catalog, maps: &[MapStats], max: usize) {
    let p = pal(ui.ctx());
    if maps.is_empty() {
        ui.label(RichText::new("No maps yet.").color(p.weak));
        return;
    }
    for m in maps.iter().take(max) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
        let art = Rect::from_min_size(rect.min + vec2(0.0, 4.0), vec2(64.0, 36.0));
        theme::image_cover(ui, art, cat.map(&m.map_id).map(|x| x.splash.as_str()), 16.0 / 9.0, 8);
        let painter = ui.painter();
        let x = art.right() + 12.0;
        let t = &m.totals;
        painter.text(
            pos2(x, rect.top() + 7.0),
            Align2::LEFT_TOP,
            cat.map_name(&m.map_id),
            theme::semibold(13.0),
            p.text,
        );
        painter.text(
            pos2(x, rect.top() + 25.0),
            Align2::LEFT_TOP,
            format!("{} · {}W {}L", games(t.matches), t.wins, t.losses),
            FontId::proportional(11.0),
            p.weak,
        );
        let wr = t.win_rate().unwrap_or(0.0);
        let bar_w = (rect.width() * 0.25).clamp(50.0, 150.0);
        let bar = Rect::from_min_size(
            pos2(rect.right() - 50.0 - bar_w, rect.center().y - 2.0),
            vec2(bar_w, 4.0),
        );
        painter.rect_filled(bar, 2.0, p.well);
        let c = if wr >= 50.0 { p.win } else { p.loss };
        painter.rect_filled(
            Rect::from_min_size(bar.min, vec2(bar.width() * wr / 100.0, 4.0)),
            2.0,
            c,
        );
        painter.text(
            pos2(rect.right(), rect.center().y),
            Align2::RIGHT_CENTER,
            format!("{wr:.0}%"),
            theme::semibold(13.0),
            c,
        );
    }
}

/// "1 game", "3 games".
pub fn games(n: u32) -> String {
    if n == 1 {
        "1 game".into()
    } else {
        format!("{n} games")
    }
}

pub fn agent_name(cat: &Catalog, id: &str) -> String {
    cat.agent(id)
        .map(|a| a.name.clone())
        .unwrap_or_else(|| "Unknown".into())
}

/// Initials on a placeholder (no art loaded).
pub fn initials(painter: &egui::Painter, rect: Rect, name: &str, p: &Palette) {
    let s: String = name.chars().take(2).collect();
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        s.to_uppercase(),
        theme::semibold(rect.height() * 0.36),
        p.weak,
    );
}

// --------------------------------------------------------------- accolades

pub fn accolade_icon(a: Accolade) -> Icon {
    match a {
        Accolade::MatchMvp => Icon::Crown,
        Accolade::TeamMvp => Icon::Star,
        Accolade::Ace => Icon::Skull,
        Accolade::Clutch => Icon::Heart,
        Accolade::QuadKill | Accolade::TripleKill => Icon::Lightning,
        Accolade::TopFragger => Icon::Swords,
        Accolade::DamageDealer => Icon::Fire,
        Accolade::EntryKing => Icon::Flag,
        Accolade::Headhunter => Icon::Target,
        Accolade::Unshakeable => Icon::Shield,
        Accolade::SpikeSpecialist => Icon::Bomb,
    }
}

pub fn rarity_color(r: Rarity, p: &Palette) -> Color32 {
    match r {
        Rarity::Common => p.weak.lerp_to_gamma(p.text, 0.3),
        Rarity::Rare => Color32::from_rgb(78, 168, 255),
        Rarity::Epic => p.accent2,
        Rarity::Legendary => p.gold,
    }
}

pub fn rarity_name(r: Rarity) -> &'static str {
    match r {
        Rarity::Common => "Common",
        Rarity::Rare => "Rare",
        Rarity::Epic => "Epic",
        Rarity::Legendary => "Legendary",
    }
}

/// A round medal: tinted disc, coloured ring and the accolade's icon.
/// Greyed out when `earned` is false.
pub fn medal(painter: &egui::Painter, center: egui::Pos2, radius: f32, a: Accolade, earned: bool, p: &Palette) {
    let c = if earned { rarity_color(a.rarity(), p) } else { p.faint };
    if earned {
        theme::radial_glow(painter, center, radius * 1.9, c.gamma_multiply(0.22));
    }
    painter.circle_filled(center, radius, if earned { c.gamma_multiply(0.16) } else { p.well });
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(2.0_f32, c.gamma_multiply(if earned { 0.9 } else { 0.4 })),
    );
    painter.circle_stroke(center, radius - 5.0, Stroke::new(1.0_f32, c.gamma_multiply(0.25)));
    let icon = Rect::from_center_size(center, vec2(radius * 0.95, radius * 0.95));
    icons::paint(painter, icon, c, accolade_icon(a));
}

/// A small inline accolade pill (icon + name).
pub fn accolade_pill(ui: &mut Ui, a: Accolade) -> egui::Response {
    let p = pal(ui.ctx());
    let c = rarity_color(a.rarity(), &p);
    let galley = ui
        .painter()
        .layout_no_wrap(a.name().to_string(), theme::medium(11.0), c);
    let size = vec2(galley.size().x + 32.0, 24.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(r, CornerRadius::same(12), c.gamma_multiply(0.14));
    icons::paint(
        ui.painter(),
        Rect::from_center_size(r.left_center() + vec2(13.0, 0.0), vec2(12.0, 12.0)),
        c,
        accolade_icon(a),
    );
    ui.painter()
        .galley(pos2(r.left() + 24.0, r.center().y - galley.size().y / 2.0), galley, c);
    resp.on_hover_text(a.description())
}

/// "3h 12m" from milliseconds.
pub fn fmt_play_time(ms: u64) -> String {
    let m = ms / 60_000;
    if m >= 60 {
        format!("{}h {:02}m", m / 60, m % 60)
    } else {
        format!("{m}m")
    }
}

/// Small key/value grid of secondary totals.
pub fn detail_grid(ui: &mut Ui, t: &Totals) {
    let p = pal(ui.ctx());
    let cells = [
        ("Kills", t.kills.to_string()),
        ("Deaths", t.deaths.to_string()),
        ("Assists", t.assists.to_string()),
        ("Kills / round", format!("{:.2}", t.kills_per_round())),
        ("First bloods", t.first_kills.to_string()),
        (
            "Opening duels",
            t.entry_pct()
                .map(|v| format!("{v:.0}% won"))
                .unwrap_or_else(|| "–".into()),
        ),
        ("Clutches", t.clutches.to_string()),
        ("Aces", t.aces.to_string()),
        ("3K / 4K rounds", format!("{} / {}", t.multi3, t.multi4)),
        ("Plants / defuses", format!("{} / {}", t.plants, t.defuses)),
        ("Rounds", t.rounds.to_string()),
        ("Time played", fmt_play_time(t.play_ms)),
    ];
    theme::tiles(ui, &cells, 120.0, 10.0, |ui, (k, v)| {
        egui::Frame::NONE
            .fill(p.well.gamma_multiply(0.6))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                theme::stat(ui, k, v, None);
            });
    });
}
