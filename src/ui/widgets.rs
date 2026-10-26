//! Small shared drawing helpers.

use egui::{Color32, Pos2, Stroke, Vec2};

use crate::content::Catalog;

pub const WIN: Color32 = Color32::from_rgb(80, 190, 140);
pub const LOSS: Color32 = Color32::from_rgb(220, 90, 90);
pub const DRAW: Color32 = Color32::from_rgb(170, 170, 170);

/// Draw an image URI at a fixed size, or reserve the space if unknown.
pub fn icon(ui: &mut egui::Ui, uri: Option<&str>, size: f32) -> egui::Response {
    match uri.filter(|u| !u.is_empty()) {
        Some(u) => ui.add(egui::Image::new(u.to_string()).fit_to_exact_size(Vec2::splat(size))),
        None => ui.allocate_response(Vec2::splat(size), egui::Sense::hover()),
    }
}

pub fn agent_icon(ui: &mut egui::Ui, cat: &Catalog, agent_id: &str, size: f32) -> egui::Response {
    let a = cat.agent(agent_id);
    let r = icon(ui, a.map(|a| a.icon.as_str()), size);
    match a {
        Some(a) => r.on_hover_text(&a.name),
        None => r,
    }
}

pub fn rank_icon(ui: &mut egui::Ui, cat: &Catalog, tier: u32, size: f32) -> egui::Response {
    icon(ui, cat.tier(tier).map(|t| t.icon.as_str()), size).on_hover_text(cat.tier_name(tier))
}

/// A minimal line chart for the RR history.
pub fn sparkline(ui: &mut egui::Ui, values: &[i32], size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    if values.len() < 2 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "not enough ranked games",
            egui::FontId::proportional(11.0),
            ui.visuals().weak_text_color(),
        );
        return;
    }
    let (min, max) = values
        .iter()
        .fold((i32::MAX, i32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let span = (max - min).max(1) as f32;
    let pad = 6.0;
    let pts: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = rect.left() + pad + (rect.width() - 2.0 * pad) * i as f32 / (values.len() - 1) as f32;
            let y = rect.bottom() - pad - (rect.height() - 2.0 * pad) * (v - min) as f32 / span;
            Pos2::new(x, y)
        })
        .collect();
    let rising = values.last() >= values.first();
    painter.add(egui::Shape::line(
        pts.clone(),
        Stroke::new(2.0_f32, if rising { WIN } else { LOSS }),
    ));
    for p in pts {
        painter.circle_filled(p, 2.0, ui.visuals().text_color());
    }
}

pub fn fmt_date(unix: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(unix, 0)
        .single()
        .map(|d| d.format("%b %d %H:%M").to_string())
        .unwrap_or_default()
}

pub fn fmt_duration_ms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}
