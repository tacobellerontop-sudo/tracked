//! 2D replay screen: the match drawn top-down on the map's minimap and
//! played like a video, with round chips, a scrubbable timeline and a kill
//! feed. Positions are exact at each kill / plant / defuse and smoothed in
//! between (see `replay::timeline`). Ability objects and dropped weapons
//! from VALORANT's replay file, when there is one, are drawn exactly where
//! and while they existed. Everything is painted with `Painter`.

use std::sync::Arc;

use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui};

use super::icons::{self, Icon};
use super::theme::{self, pal, Palette};
use crate::app::{AppState, Screen};
use crate::content::valorant_api::queue_name;
use crate::content::Catalog;
use crate::ipc::IntentTx;
use crate::replay::calibrate::{facing, world_to_norm, MapCal};
use crate::replay::timeline::RoundTimeline;
use crate::replay::{Availability, Frame, FrameKind, ItemKind, Loc, MapItem, MatchReplay, RoundReplay};

const FEED_W: f32 = 300.0;
const ALLY: Color32 = Color32::from_rgb(38, 186, 168);
const ENEMY: Color32 = Color32::from_rgb(236, 72, 88);
const SPIKE: Color32 = Color32::from_rgb(255, 196, 0);
/// How long a kill line stays on screen after the kill (round ms).
const KILL_FLASH: f32 = 1600.0;

pub fn ui(ui: &mut Ui, state: &mut AppState, intents: &IntentTx) {
    let p = pal(ui.ctx());
    header(ui, state, &p);
    let Some(data) = state.replay.data.clone() else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.spinner();
            ui.label(RichText::new("Loading match…").color(p.weak));
        });
        return;
    };
    let m = match data {
        Ok(m) => m,
        Err(e) => {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Couldn't load this match")
                        .font(theme::semibold(15.0))
                        .color(p.text),
                );
                ui.label(RichText::new(e).color(p.weak));
                ui.add_space(8.0);
                if ui.button("Try again").clicked() {
                    if let Some(id) = state.replay.match_id.clone() {
                        state.replay.data = None;
                        intents.send(crate::ipc::Intent::OpenReplay(id));
                    }
                }
            });
            return;
        }
    };
    if let Availability::UnsupportedMode(mode) = &m.availability {
        empty(
            ui,
            &p,
            &format!("Replay isn't available for {mode}"),
            "It needs rounds and sides, which this mode doesn't have.",
        );
        return;
    }
    if m.rounds.is_empty() {
        empty(ui, &p, "No rounds in this match", "There's nothing to replay.");
        return;
    }
    let cat = state.catalog.clone();
    let my_team = my_team(state, &m);
    state.replay.round = state.replay.round.min(m.rounds.len() - 1);
    keyboard(ui, state, &m);
    playback(ui, state, &m);

    round_chips(ui, state, &m, &my_team, &p);
    ui.add_space(10.0);

    let round = &m.rounds[state.replay.round];
    let avail = ui.available_size();
    // Leave room under the map for the timeline and controls.
    let side = (avail.x - FEED_W - 20.0).min(avail.y - 96.0).max(260.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(side);
            map_canvas(ui, state, &m, round, &cat, &my_team, side, &p);
            ui.add_space(8.0);
            controls(ui, state, &m, &p);
        });
        ui.add_space(20.0);
        ui.vertical(|ui| {
            ui.set_width((avail.x - side - 20.0).max(FEED_W));
            feed(ui, state, &m, &cat, &my_team, &p);
        });
    });
}

fn header(ui: &mut Ui, state: &mut AppState, p: &Palette) {
    let data = state.replay.data.clone().and_then(Result::ok);
    let cat = state.catalog.clone();
    ui.horizontal(|ui| {
        let back = ui.add(
            egui::Button::new(
                RichText::new(Icon::Back.glyph())
                    .font(theme::icon_font(16.0))
                    .color(p.weak),
            )
            .frame(false),
        );
        if back.on_hover_text("Back").clicked() {
            state.replay.playing = false;
            state.screen = state.replay.return_to.unwrap_or(Screen::Matches);
        }
        icons::icon(ui, Icon::Map, 18.0, p.accent);
        let title = match &data {
            Some(m) => format!("{} · {}", cat.map_name(&m.map_id), queue_name(&m.queue_id)),
            None => "2D replay".into(),
        };
        ui.label(RichText::new(title).font(theme::semibold(18.0)).color(p.text));
        if let Some(m) = &data {
            let mine = my_team(state, m);
            let score = |team: &str| {
                m.teams
                    .iter()
                    .find(|t| t.team == team)
                    .map(|t| t.rounds_won)
                    .unwrap_or(0)
            };
            let other = if mine == "Red" { "Blue" } else { "Red" };
            let (a, b) = (score(&mine), score(other));
            let color = if a > b {
                p.win
            } else if a < b {
                p.loss
            } else {
                p.weak
            };
            ui.label(
                RichText::new(format!("{a} – {b}"))
                    .font(theme::semibold(16.0))
                    .color(color),
            );
        }
    });
    // Which movement source this replay uses.
    let note = match data.as_ref().and_then(|m| m.replay_file.as_deref()) {
        Some(file) => format!(
            "Abilities and dropped weapons: exact, from your downloaded replay ({file}). \
             Player movement: smoothed between recorded moments (the replay file's movement is encrypted)."
        ),
        None => "Movement: smoothed between recorded moments (Riot's match data only has positions at each kill, \
                 plant and defuse). Not tracked."
            .to_string(),
    };
    ui.label(RichText::new(note).size(11.5).color(p.faint));
    ui.add_space(8.0);
}

fn empty(ui: &mut Ui, p: &Palette, title: &str, body: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        icons::icon(ui, Icon::Map, 40.0, p.faint);
        ui.add_space(8.0);
        ui.label(RichText::new(title).font(theme::semibold(16.0)).color(p.text));
        ui.label(RichText::new(body).color(p.weak));
    });
}

/// The team drawn as "yours": the perspective player's, else Blue.
fn my_team(state: &AppState, m: &MatchReplay) -> String {
    state
        .replay
        .perspective
        .as_deref()
        .and_then(|p| m.player(p))
        .map(|p| p.team.clone())
        .unwrap_or_else(|| "Blue".into())
}

fn team_color(team: &str, mine: &str) -> Color32 {
    if team == mine {
        ALLY
    } else {
        ENEMY
    }
}

/// The playback clock for the current round, defaulting to its start.
fn now_t(state: &AppState, m: &MatchReplay) -> f32 {
    let tl = &m.rounds[state.replay.round].timeline;
    state.replay.t.unwrap_or(tl.start).clamp(tl.start, tl.end)
}

fn keyboard(ui: &mut Ui, state: &mut AppState, m: &MatchReplay) {
    if ui.ctx().wants_keyboard_input() {
        return;
    }
    let (left, right, up, down, space) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Space),
        )
    });
    if left {
        seek_event(state, m, -1);
    }
    if right {
        seek_event(state, m, 1);
    }
    if up {
        jump_round(state, m, -1);
    }
    if down {
        jump_round(state, m, 1);
    }
    if space {
        toggle_play(state, m);
    }
}

fn toggle_play(state: &mut AppState, m: &MatchReplay) {
    // Pressing play at the very end restarts the round.
    let tl = &m.rounds[state.replay.round].timeline;
    if !state.replay.playing && now_t(state, m) >= tl.end - 1.0 {
        state.replay.t = Some(tl.start);
    }
    state.replay.playing = !state.replay.playing;
}

/// Jump to the previous / next recorded moment (kill, plant, defuse),
/// crossing into neighbouring rounds at the ends.
fn seek_event(state: &mut AppState, m: &MatchReplay, dir: i32) {
    let t = now_t(state, m);
    let tl = &m.rounds[state.replay.round].timeline;
    if dir > 0 {
        match tl.events.iter().find(|e| e.0 > t + 1.0) {
            Some(e) => state.replay.t = Some(e.0),
            None if state.replay.round + 1 < m.rounds.len() => jump_round(state, m, 1),
            None => state.replay.t = Some(tl.end),
        }
    } else {
        match tl.events.iter().rev().find(|e| e.0 < t - 1.0) {
            Some(e) => state.replay.t = Some(e.0),
            None if t > tl.start + 1.0 => state.replay.t = Some(tl.start),
            None if state.replay.round > 0 => {
                jump_round(state, m, -1);
                state.replay.t = Some(m.rounds[state.replay.round].timeline.end);
            }
            None => {}
        }
    }
}

fn jump_round(state: &mut AppState, m: &MatchReplay, dir: i32) {
    let r = &mut state.replay;
    r.round = (r.round as i32 + dir).clamp(0, m.rounds.len() as i32 - 1) as usize;
    r.t = None;
}

/// Advance the clock in real time while playing; roll into the next round.
fn playback(ui: &Ui, state: &mut AppState, m: &MatchReplay) {
    if !state.replay.playing {
        return;
    }
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let t = now_t(state, m) + dt * 1000.0 * state.replay.speed;
    let tl = &m.rounds[state.replay.round].timeline;
    if t >= tl.end {
        if state.replay.round + 1 < m.rounds.len() {
            jump_round(state, m, 1);
        } else {
            state.replay.t = Some(tl.end);
            state.replay.playing = false;
        }
    } else {
        state.replay.t = Some(t);
    }
    ui.ctx().request_repaint();
}

fn round_chips(ui: &mut Ui, state: &mut AppState, m: &MatchReplay, mine: &str, p: &Palette) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (i, r) in m.rounds.iter().enumerate() {
            if i == 12 {
                ui.add_space(8.0); // halftime
            }
            let won = r.winning_team == mine;
            let selected = i == state.replay.round;
            let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
            let base = if won { p.win } else { p.loss };
            let fill = if selected { base } else { base.gamma_multiply(0.18) };
            ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
            if resp.hovered() && !selected {
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(6),
                    Stroke::new(1.0_f32, base),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                (i + 1).to_string(),
                theme::medium(11.0),
                if selected { Color32::WHITE } else { p.text },
            );
            let tip = format!(
                "Round {} · {} · {}",
                i + 1,
                if won { "won" } else { "lost" },
                if r.result.is_empty() { "—" } else { r.result.as_str() }
            );
            if resp.on_hover_text(tip).clicked() {
                state.replay.round = i;
                state.replay.t = None;
            }
        }
    });
}

/// Screen position of a world location inside the map rect.
fn to_screen(l: &Loc, cal: &MapCal, rect: Rect) -> Pos2 {
    let (nx, ny) = world_to_norm(l, cal);
    pos2(rect.left() + nx * rect.width(), rect.top() + ny * rect.height())
}

#[allow(clippy::too_many_arguments)]
fn map_canvas(
    ui: &mut Ui,
    state: &AppState,
    m: &MatchReplay,
    round: &RoundReplay,
    cat: &Arc<Catalog>,
    mine: &str,
    side: f32,
    p: &Palette,
) {
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(14), p.well);
    let map = cat.map(&m.map_id);
    let minimap = map.map(|m| m.minimap.as_str()).unwrap_or_default();
    if minimap.is_empty() {
        // No artwork (offline / demo): a faint grid so positions still read.
        for i in 1..8 {
            let t = i as f32 / 8.0;
            let s = Stroke::new(1.0_f32, p.grid);
            painter.line_segment(
                [
                    pos2(rect.left() + t * side, rect.top()),
                    pos2(rect.left() + t * side, rect.bottom()),
                ],
                s,
            );
            painter.line_segment(
                [
                    pos2(rect.left(), rect.top() + t * side),
                    pos2(rect.right(), rect.top() + t * side),
                ],
                s,
            );
        }
        painter.text(
            rect.left_bottom() + vec2(12.0, -12.0),
            Align2::LEFT_BOTTOM,
            "No minimap artwork available",
            theme::medium(11.0),
            p.faint,
        );
    } else {
        egui::Image::new(minimap).paint_at(ui, rect);
    }
    let Some(cal) = map.and_then(|m| m.cal) else {
        overlay_note(
            &painter,
            rect,
            p,
            "No calibration for this map yet: showing the kill feed only.",
        );
        return;
    };
    if m.availability == Availability::NoPositions {
        overlay_note(
            &painter,
            rect,
            p,
            "This match has no position data: showing the kill feed only.",
        );
        return;
    }
    let tl = &round.timeline;
    if tl.tracks.is_empty() {
        let text = if round.feed.is_empty() {
            "No kills this round".to_string()
        } else {
            "No position data for this round's kills".to_string()
        };
        overlay_note(&painter, rect, p, &format!("{text} · {}", outcome(round, mine)));
        return;
    }
    let t = now_t(state, m);
    draw_moment(ui, &painter, round, tl, t, m, cat, &cal, rect, mine, resp.hover_pos());
    // Round clock in the corner, like a broadcast overlay.
    let label = format!("Round {}  ·  {}", state.replay.round + 1, clock(t.max(0.0) as u32));
    let g = painter.layout_no_wrap(label, theme::semibold(12.0), Color32::WHITE);
    let r = Rect::from_min_size(rect.left_top() + vec2(12.0, 12.0), g.size() + vec2(18.0, 10.0));
    painter.rect_filled(r, CornerRadius::same(8), Color32::from_black_alpha(150));
    painter.galley(r.min + vec2(9.0, 5.0), g, Color32::WHITE);
}

fn overlay_note(painter: &egui::Painter, rect: Rect, p: &Palette, text: &str) {
    let galley = painter.layout(text.to_string(), theme::medium(12.5), p.text, rect.width() - 60.0);
    let r = Rect::from_center_size(rect.center(), galley.size() + vec2(28.0, 18.0));
    painter.rect_filled(r, CornerRadius::same(10), p.card.gamma_multiply(0.94));
    painter.galley(r.center() - galley.size() / 2.0, galley, p.text);
}

fn outcome(round: &RoundReplay, mine: &str) -> String {
    let who = if round.winning_team == mine { "won" } else { "lost" };
    match round.result.as_str() {
        "" => format!("round {who}"),
        r => format!("round {who} ({})", r.to_lowercase()),
    }
}

/// Draw the round at time `t`: smoothed player positions, kill lines that
/// flash in as kills happen, the spike once it's down.
#[allow(clippy::too_many_arguments)]
fn draw_moment(
    ui: &Ui,
    painter: &egui::Painter,
    round: &RoundReplay,
    tl: &RoundTimeline,
    t: f32,
    m: &MatchReplay,
    cat: &Catalog,
    cal: &MapCal,
    rect: Rect,
    mine: &str,
    hover: Option<Pos2>,
) {
    let scale = (rect.width() / 640.0).clamp(0.75, 1.4);
    let r = 11.0 * scale;
    let poses = tl.poses_at(t);
    let item_tip = draw_items(ui, painter, round, t, m, cat, cal, rect, mine, scale, hover);
    let screen_of = |puuid: &str| {
        poses
            .iter()
            .find(|x| x.puuid == puuid)
            .map(|x| to_screen(&x.pos, cal, rect))
    };

    // Spike: from the plant on; the defuse marker once defused.
    for f in round
        .frames
        .iter()
        .filter(|f| f.kind != FrameKind::Kill && f.t_ms as f32 <= t)
    {
        if let Some(mk) = &f.marker {
            let age = t - f.t_ms as f32;
            let pulse = if age < 1200.0 {
                1.0 + 0.35 * (1.0 - age / 1200.0)
            } else {
                1.0
            };
            spike_marker(painter, mk, cal, rect, scale * pulse, 1.0);
        }
    }

    // Recent kills: a line from the killer (where they are now) to where the
    // victim fell, fading out.
    let mut flashing: Vec<(&str, &str, f32, bool)> = Vec::new();
    for f in round.frames.iter().filter(|f| f.kind == FrameKind::Kill) {
        let age = t - f.t_ms as f32;
        if !(0.0..KILL_FLASH).contains(&age) {
            continue;
        }
        let Some(k) = &f.kill else { continue };
        let alpha = 1.0 - age / KILL_FLASH;
        flashing.push((k.killer.as_str(), k.victim.as_str(), alpha, k.headshot));
        if let (Some(a), Some(b)) = (screen_of(&k.killer), screen_of(&k.victim)) {
            let team = m.player(&k.killer).map(|x| x.team.as_str()).unwrap_or_default();
            let c = team_color(team, mine).gamma_multiply(0.9 * alpha);
            painter.line_segment([a, b], Stroke::new(2.5_f32 * scale, c));
            if a.distance(b) > r * 2.0 {
                let dir = (b - a).normalized();
                let tip = b - dir * (r + 2.0);
                let n = vec2(-dir.y, dir.x);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        tip,
                        tip - dir * 9.0 * scale + n * 5.0 * scale,
                        tip - dir * 9.0 * scale - n * 5.0 * scale,
                    ],
                    c,
                    Stroke::NONE,
                ));
            }
        }
    }

    let mut hovered = None;
    // Dead first so the living draw on top.
    for alive in [false, true] {
        for pl in poses.iter().filter(|x| x.alive == alive) {
            let c = to_screen(&pl.pos, cal, rect);
            let color = team_color(pl.team, mine);
            if hover.is_some_and(|h| h.distance(c) <= r + 3.0) {
                hovered = Some(pl);
            }
            if !alive {
                let flash = flashing.iter().find(|f| f.1 == pl.puuid);
                let (s, ink) = match flash {
                    Some(_) => (6.5 * scale, Color32::WHITE),
                    None => (4.5 * scale, color.gamma_multiply(0.6)),
                };
                if let Some(f) = flash {
                    painter.circle_filled(c, r * (1.0 + 0.6 * f.2), color.gamma_multiply(0.35 * f.2));
                    painter.circle_filled(c, r * 0.85, Color32::from_black_alpha(150));
                }
                let st = Stroke::new(2.2_f32 * scale, ink);
                painter.line_segment([c + vec2(-s, -s), c + vec2(s, s)], st);
                painter.line_segment([c + vec2(-s, s), c + vec2(s, -s)], st);
                if flash.is_some_and(|f| f.3) {
                    headshot_mark(painter, c + vec2(r + 6.0, -r - 4.0), scale);
                }
                continue;
            }
            if let Some(theta) = pl.facing {
                let (dx, dy) = facing(theta, cal);
                let d = vec2(dx, dy);
                painter.line_segment(
                    [c + d * r, c + d * (r + 9.0 * scale)],
                    Stroke::new(3.0_f32 * scale, color),
                );
            }
            painter.circle_filled(c, r + 2.0, Color32::from_black_alpha(150));
            painter.circle_filled(c, r, color);
            let agent = cat.agent(pl.agent);
            match agent.map(|a| a.icon.as_str()).filter(|u| !u.is_empty()) {
                Some(uri) => {
                    let ir = Rect::from_center_size(c, vec2(2.0 * r - 3.0, 2.0 * r - 3.0));
                    egui::Image::new(uri)
                        .corner_radius(CornerRadius::same((r - 1.5) as u8))
                        .paint_at(ui, ir);
                }
                None => {
                    let letter = agent
                        .map(|a| a.name.chars().next().unwrap_or('?'))
                        .or_else(|| m.player(pl.puuid).and_then(|x| x.name.chars().next()))
                        .unwrap_or('?');
                    painter.text(
                        c,
                        Align2::CENTER_CENTER,
                        letter,
                        theme::semibold(11.0 * scale),
                        Color32::WHITE,
                    );
                }
            }
            if let Some(f) = flashing.iter().find(|f| f.0 == pl.puuid) {
                painter.circle_stroke(c, r + 3.5, Stroke::new(2.0_f32, Color32::WHITE.gamma_multiply(f.2)));
            }
        }
    }
    if hovered.is_none() {
        if let Some(text) = item_tip {
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("replay-item"), |ui| {
                ui.label(text);
            });
        }
    }
    if let Some(pl) = hovered {
        let name = m
            .player(pl.puuid)
            .map(|x| format!("{}#{}", x.name, x.tag))
            .unwrap_or_default();
        let agent = cat.agent(pl.agent).map(|a| a.name.clone()).unwrap_or_default();
        let text = format!("{name}\n{agent}{}", if pl.alive { "" } else { " · dead" });
        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("replay-dot"), |ui| {
            ui.label(text);
        });
    }
}

/// Team of the only player on `agent` (Riot's internal name), if just one
/// player picked it.
fn agent_team<'a>(m: &'a MatchReplay, cat: &Catalog, agent: &str) -> Option<&'a str> {
    let (id, _) = cat.agent_by_dev_name(agent)?;
    let mut on = m.players.iter().filter(|p| p.agent.eq_ignore_ascii_case(id));
    match (on.next(), on.next()) {
        (Some(p), None) => Some(p.team.as_str()),
        _ => None,
    }
}

fn item_alive(it: &MapItem, t: f32) -> bool {
    it.t <= t && it.end.is_none_or(|e| t < e)
}

/// Ability objects (agent icon in a team-coloured ring) and dropped weapons
/// (small diamonds) that exist at time `t`. Returns the hovered one's label.
#[allow(clippy::too_many_arguments)]
fn draw_items(
    ui: &Ui,
    painter: &egui::Painter,
    round: &RoundReplay,
    t: f32,
    m: &MatchReplay,
    cat: &Catalog,
    cal: &MapCal,
    rect: Rect,
    mine: &str,
    scale: f32,
    hover: Option<Pos2>,
) -> Option<String> {
    let mut tip = None;
    for it in round.items.iter().filter(|it| item_alive(it, t)) {
        let c = to_screen(&it.pos, cal, rect);
        let fade = ((t - it.t) / 300.0).clamp(0.0, 1.0);
        match &it.kind {
            ItemKind::DroppedWeapon => {
                let s = 4.5 * scale;
                let pts = vec![c + vec2(0.0, -s), c + vec2(s, 0.0), c + vec2(0.0, s), c + vec2(-s, 0.0)];
                painter.add(egui::Shape::convex_polygon(
                    pts,
                    Color32::from_gray(230).gamma_multiply(0.9 * fade),
                    Stroke::new(1.5_f32, Color32::from_black_alpha((160.0 * fade) as u8)),
                ));
                if hover.is_some_and(|h| h.distance(c) <= s + 3.0) {
                    tip = Some("Dropped weapon".to_string());
                }
            }
            ItemKind::Ability { agent, name } => {
                let r = 7.0 * scale;
                let color = match agent_team(m, cat, agent) {
                    Some(team) => team_color(team, mine),
                    None => Color32::from_gray(200),
                };
                painter.circle_filled(c, r * 1.9, color.gamma_multiply(0.16 * fade));
                painter.circle_filled(c, r + 1.5, Color32::from_black_alpha((170.0 * fade) as u8));
                painter.circle_stroke(c, r, Stroke::new(1.8_f32, color.gamma_multiply(fade)));
                let info = cat.agent_by_dev_name(agent).map(|(_, a)| a);
                match info.map(|a| a.icon.as_str()).filter(|u| !u.is_empty()) {
                    Some(uri) => {
                        let ir = Rect::from_center_size(c, vec2(2.0 * r - 3.0, 2.0 * r - 3.0));
                        egui::Image::new(uri)
                            .tint(Color32::WHITE.gamma_multiply(fade))
                            .corner_radius(CornerRadius::same((r - 1.5) as u8))
                            .paint_at(ui, ir);
                    }
                    None => {
                        painter.circle_filled(c, r - 1.5, color.gamma_multiply(0.45 * fade));
                        let letter = info.and_then(|a| a.name.chars().next()).unwrap_or('•');
                        painter.text(
                            c,
                            Align2::CENTER_CENTER,
                            letter,
                            theme::semibold(9.0 * scale),
                            Color32::WHITE.gamma_multiply(fade),
                        );
                    }
                }
                if hover.is_some_and(|h| h.distance(c) <= r + 3.0) {
                    let who = info.map(|a| a.name.as_str()).unwrap_or(agent);
                    tip = Some(format!("{who} · {name}"));
                }
            }
        }
    }
    tip
}

/// Diamond at the plant (yellow) or defuse (teal) spot, with the site letter.
fn spike_marker(painter: &egui::Painter, mk: &crate::replay::Marker, cal: &MapCal, rect: Rect, scale: f32, alpha: f32) {
    let c = to_screen(&mk.pos, cal, rect);
    let s = 9.0 * scale;
    let color = if mk.kind == FrameKind::Defuse { ALLY } else { SPIKE }.gamma_multiply(alpha);
    painter.add(egui::Shape::convex_polygon(
        vec![c + vec2(0.0, -s), c + vec2(s, 0.0), c + vec2(0.0, s), c + vec2(-s, 0.0)],
        color,
        Stroke::new(1.5_f32, Color32::from_black_alpha((160.0 * alpha) as u8)),
    ));
    if let Some(site) = &mk.site {
        painter.text(
            c + vec2(0.0, -s - 8.0),
            Align2::CENTER_BOTTOM,
            site,
            theme::semibold(12.0 * scale),
            color,
        );
    }
}

/// Small crosshair glyph for "headshot".
fn headshot_mark(painter: &egui::Painter, c: Pos2, scale: f32) {
    let s = Stroke::new(1.6_f32, SPIKE);
    let r = 5.0 * scale;
    painter.circle_filled(c, r + 2.5, Color32::from_black_alpha(160));
    painter.circle_stroke(c, r, s);
    for d in [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)] {
        painter.line_segment([c + d * (r - 2.5), c + d * (r + 2.5)], s);
    }
}

fn controls(ui: &mut Ui, state: &mut AppState, m: &MatchReplay, p: &Palette) {
    timeline_bar(ui, state, m, p);
    ui.add_space(6.0);
    let t = now_t(state, m);
    let tl = &m.rounds[state.replay.round].timeline;
    ui.horizontal(|ui| {
        let button = |ui: &mut Ui, icon: Icon, tip: &str, enabled: bool| {
            let color = if enabled { p.text } else { p.faint };
            let b = egui::Button::new(RichText::new(icon.glyph()).font(theme::icon_font(17.0)).color(color))
                .min_size(vec2(34.0, 30.0))
                .corner_radius(CornerRadius::same(8));
            ui.add_enabled(enabled, b).on_hover_text(tip).clicked()
        };
        let r = state.replay.round;
        if button(ui, Icon::PrevRound, "Previous round (↑)", r > 0) {
            jump_round(state, m, -1);
        }
        if button(ui, Icon::Prev, "Previous kill / event (←)", r > 0 || t > tl.start + 1.0) {
            seek_event(state, m, -1);
        }
        let playing = state.replay.playing;
        if button(
            ui,
            if playing { Icon::Pause } else { Icon::Play },
            "Play / pause (Space)",
            true,
        ) {
            toggle_play(state, m);
        }
        let at_end = r + 1 == m.rounds.len() && t >= tl.end - 1.0;
        if button(ui, Icon::Next, "Next kill / event (→)", !at_end) {
            seek_event(state, m, 1);
        }
        if button(ui, Icon::NextRound, "Next round (↓)", r + 1 < m.rounds.len()) {
            jump_round(state, m, 1);
        }
        ui.add_space(6.0);
        for speed in [0.5_f32, 1.0, 2.0, 4.0] {
            if theme::pill(ui, &format!("{speed}×"), state.replay.speed == speed).clicked() {
                state.replay.speed = speed;
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} / {}", clock(t.max(0.0) as u32), clock(tl.end as u32)))
                    .font(theme::medium(12.0))
                    .color(p.weak),
            );
        });
    });
}

/// Video-style scrub bar: progress, a tick per kill / plant / defuse, and a
/// draggable playhead.
fn timeline_bar(ui: &mut Ui, state: &mut AppState, m: &MatchReplay, p: &Palette) {
    let round = &m.rounds[state.replay.round];
    let tl = &round.timeline;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click_and_drag());
    let track = Rect::from_center_size(rect.center(), vec2(rect.width() - 12.0, 6.0));
    let x_of = |t: f32| track.left() + (t - tl.start) / tl.duration() * track.width();
    let t_of = |x: f32| tl.start + ((x - track.left()) / track.width()).clamp(0.0, 1.0) * tl.duration();
    if let Some(pos) = resp.interact_pointer_pos() {
        state.replay.t = Some(t_of(pos.x));
    }
    let t = now_t(state, m);
    let painter = ui.painter();
    painter.rect_filled(track, CornerRadius::same(3), p.well);
    let done = Rect::from_min_max(track.min, pos2(x_of(t), track.max.y));
    painter.rect_filled(done, CornerRadius::same(3), p.accent.gamma_multiply(0.55));
    let mine = my_team(state, m);
    for (et, kind, i) in &tl.events {
        let color = match kind {
            FrameKind::Kill => {
                let killer = round.frames[*i].kill.as_ref().map(|k| k.killer.as_str()).unwrap_or("");
                team_color(m.player(killer).map(|x| x.team.as_str()).unwrap_or(""), &mine)
            }
            FrameKind::Plant => SPIKE,
            FrameKind::Defuse => ALLY,
        };
        let x = x_of(*et);
        painter.line_segment(
            [pos2(x, track.top() - 5.0), pos2(x, track.bottom() + 5.0)],
            Stroke::new(2.0_f32, color),
        );
    }
    let knob = pos2(x_of(t), track.center().y);
    painter.circle_filled(knob, 7.0, p.card);
    painter.circle_stroke(knob, 7.0, Stroke::new(2.0_f32, p.accent));
    if let Some(h) = resp.hover_pos() {
        resp.on_hover_text_at_pointer(clock(t_of(h.x).max(0.0) as u32));
    }
}

fn clock(ms: u32) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}

fn weapon_label(cat: &Catalog, k: &crate::replay::KillLink) -> (String, Option<String>) {
    let d = &k.damage;
    match d.damage_type.as_str() {
        "Bomb" => ("Spike".into(), None),
        "Fall" => ("Fall".into(), None),
        "Melee" => ("Melee".into(), None),
        "Ability" => {
            let name = match d.damage_item.as_str() {
                "Ultimate" => "Ultimate",
                "GrenadeAbility" => "Ability (Q/C)",
                "Ability1" | "Ability2" => "Ability",
                _ => "Ability",
            };
            (name.into(), None)
        }
        _ => match cat.weapon(&d.damage_item) {
            Some(w) => (w.name.clone(), Some(w.icon.clone()).filter(|u| !u.is_empty())),
            None if d.damage_item.is_empty() => ("—".into(), None),
            None => ("Weapon".into(), None),
        },
    }
}

fn feed(ui: &mut Ui, state: &mut AppState, m: &MatchReplay, cat: &Catalog, mine: &str, p: &Palette) {
    let round = &m.rounds[state.replay.round];
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Round {}", state.replay.round + 1))
                .font(theme::semibold(15.0))
                .color(p.text),
        );
        let side = round
            .attackers
            .as_deref()
            .map(|a| if a == mine { "you attack" } else { "you defend" });
        if let Some(side) = side {
            ui.label(RichText::new(side).size(11.5).color(p.weak));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let won = round.winning_team == mine;
            ui.label(
                RichText::new(if won { "Won" } else { "Lost" })
                    .font(theme::semibold(12.5))
                    .color(if won { p.win } else { p.loss }),
            );
        });
    });
    if !round.result.is_empty() {
        ui.label(RichText::new(&round.result).size(11.5).color(p.faint));
    }
    ui.add_space(6.0);
    if round.feed.is_empty() {
        ui.label(RichText::new("No kills this round.").color(p.weak));
    }
    let name = |puuid: &str| m.player(puuid).map(|x| x.name.clone()).unwrap_or_else(|| "?".into());
    let now = now_t(state, m);
    let mut jump: Option<f32> = None;
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 90.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            // Spike events in the feed too, interleaved by time.
            let mut rows: Vec<FeedRow> = round
                .feed
                .iter()
                .map(|e| (e.t_ms, e.frame, Some(e), None))
                .chain(
                    round
                        .frames
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| f.kind != FrameKind::Kill)
                        .map(|(i, f)| (f.t_ms, Some(i), None, Some(f))),
                )
                .collect();
            rows.sort_by_key(|r| r.0);
            for (t, frame, entry, spike) in rows {
                // Highlight what just happened; dim what hasn't happened yet.
                let age = now - t as f32;
                let selected = (0.0..KILL_FLASH).contains(&age);
                let upcoming = age < 0.0;
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                let mut painter = ui.painter().clone();
                if upcoming {
                    painter.multiply_opacity(0.5);
                }
                if selected {
                    painter.rect_filled(rect, CornerRadius::same(8), p.selected);
                } else if resp.hovered() && frame.is_some() {
                    painter.rect_filled(rect, CornerRadius::same(8), p.selected.gamma_multiply(0.5));
                }
                let mut x = rect.left() + 8.0;
                let y = rect.center().y;
                let text = |x: &mut f32, s: &str, font: egui::FontId, c: Color32| {
                    let g = painter.layout_no_wrap(s.to_string(), font, c);
                    let w = g.size().x;
                    painter.galley(pos2(*x, y - g.size().y / 2.0), g, c);
                    *x += w + 6.0;
                };
                text(&mut x, &clock(t), theme::medium(11.0), p.faint);
                if let Some(e) = entry {
                    let k = &e.kill;
                    let kt = m.player(&k.killer).map(|x| x.team.clone()).unwrap_or_default();
                    let vt = m.player(&k.victim).map(|x| x.team.clone()).unwrap_or_default();
                    text(&mut x, &name(&k.killer), theme::semibold(12.0), team_color(&kt, mine));
                    let (wname, wicon) = weapon_label(cat, k);
                    match wicon {
                        Some(uri) => {
                            let ir = Rect::from_min_size(pos2(x, y - 8.0), vec2(48.0, 16.0));
                            egui::Image::new(uri)
                                .tint(p.weak)
                                .fit_to_exact_size(ir.size())
                                .paint_at(ui, ir);
                            x += 52.0;
                        }
                        None => text(&mut x, &wname, theme::medium(10.5), p.weak),
                    }
                    if k.headshot {
                        headshot_mark(&painter, pos2(x + 4.0, y), 0.8);
                        x += 14.0;
                    }
                    text(&mut x, &name(&k.victim), theme::semibold(12.0), team_color(&vt, mine));
                    if e.frame.is_none() {
                        text(&mut x, "no positions", theme::medium(10.0), p.faint);
                    }
                    let tip = format!(
                        "{} killed {} with {wname}{}",
                        name(&k.killer),
                        name(&k.victim),
                        if k.headshot { " (headshot)" } else { "" }
                    );
                    if resp.on_hover_text(tip).clicked() {
                        jump = Some(t as f32);
                    }
                } else if let Some(f) = spike {
                    let mk = f.marker.as_ref();
                    let who = mk.and_then(|mk| mk.by.as_deref()).map(name).unwrap_or_default();
                    let what = match (f.kind, mk.and_then(|mk| mk.site.as_deref())) {
                        (FrameKind::Plant, Some(site)) => format!("planted on {site}"),
                        (FrameKind::Plant, None) => "planted the spike".into(),
                        _ => "defused the spike".into(),
                    };
                    text(&mut x, &who, theme::semibold(12.0), SPIKE);
                    text(&mut x, &what, theme::medium(11.5), p.weak);
                    if resp.clicked() {
                        jump = Some(t as f32);
                    }
                }
            }
        });
    // Clicking a line plays the lead-up to that moment.
    if let Some(t) = jump {
        let tl = &round.timeline;
        state.replay.t = Some((t - 2000.0).max(tl.start));
        state.replay.playing = true;
    }
    ui.add_space(10.0);
    legend(ui, p, !round.items.is_empty());
}

/// A kill-feed line: time, frame to jump to, and either a kill or a spike event.
type FeedRow<'a> = (
    u32,
    Option<usize>,
    Option<&'a crate::replay::FeedEntry>,
    Option<&'a Frame>,
);

fn legend(ui: &mut Ui, p: &Palette, items: bool) {
    let item = |ui: &mut Ui, color: Color32, label: &str| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 5.0, color);
        ui.label(RichText::new(label).size(11.0).color(p.weak));
    };
    ui.horizontal_wrapped(|ui| {
        item(ui, ALLY, "Your team");
        item(ui, ENEMY, "Enemy");
        item(ui, SPIKE, "Spike");
        ui.label(RichText::new("x = dead · white ring = killer").size(11.0).color(p.weak));
    });
    if items {
        ui.label(
            RichText::new("Small agent icons = abilities on the map · ◆ = dropped weapon (from the replay file)")
                .size(11.0)
                .color(p.weak),
        );
    }
    ui.label(
        RichText::new("Space play · ←/→ kills · ↑/↓ rounds · drag the timeline to scrub")
            .size(10.5)
            .color(p.faint),
    );
}
