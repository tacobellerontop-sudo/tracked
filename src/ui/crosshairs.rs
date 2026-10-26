//! Crosshair codes: paste, tweak, preview, save presets, copy.

use egui::{Color32, Pos2, Rect, RichText, Vec2};

use super::widgets;
use crate::app::AppState;
use crate::config::CrosshairPreset;
use crate::crosshair::{CrosshairCode, Lines, Primary, COLOR_NAMES};
use crate::ipc::IntentTx;

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.heading("Crosshairs");
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label("Code");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.crosshair.input)
                .hint_text("0;P;c;5;h;0;0l;4;0o;2;0a;1;0f;0;1b;0")
                .desired_width(420.0)
                .font(egui::TextStyle::Monospace),
        );
        if resp.changed() {
            let input = state.crosshair.input.clone();
            state.crosshair.load(&input);
        }
        if ui.button("New").clicked() {
            let mut c = CrosshairCode::default();
            c.set_primary(&CrosshairCode::default().primary());
            state.crosshair.load(&c.to_string());
        }
    });

    let parsed = state.crosshair.parsed.clone();
    match &parsed {
        None => {
            ui.weak("Paste a VALORANT crosshair code, or press New to build one.");
        }
        Some(Err(e)) => {
            ui.colored_label(widgets::LOSS, format!("Invalid code: {e}"));
        }
        Some(Ok(code)) => {
            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                preview(ui, &code.primary());
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    let mut p = code.primary();
                    if editor(ui, &mut p) {
                        let mut c = code.clone();
                        c.set_primary(&p);
                        state.crosshair.load(&c.to_string());
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Copy code").clicked() {
                    ui.ctx().copy_text(state.crosshair.input.clone());
                }
                ui.add(
                    egui::TextEdit::singleline(&mut state.crosshair.name)
                        .hint_text("Preset name")
                        .desired_width(160.0),
                );
                let can_save = !state.crosshair.name.trim().is_empty();
                if ui.add_enabled(can_save, egui::Button::new("Save preset")).clicked() {
                    let name = state.crosshair.name.trim().to_string();
                    let code = state.crosshair.input.trim().to_string();
                    let presets = &mut state.settings.crosshairs;
                    match presets.iter_mut().find(|p| p.name == name) {
                        Some(p) => p.code = code,
                        None => presets.push(CrosshairPreset { name, code }),
                    }
                    state.save_settings(intents);
                }
            });
        }
    }

    ui.add_space(14.0);
    ui.separator();
    ui.label(RichText::new("Saved presets").strong());
    if state.settings.crosshairs.is_empty() {
        ui.weak("No presets yet.");
    }
    let mut load = None;
    let mut delete = None;
    for (i, p) in state.settings.crosshairs.iter().enumerate() {
        ui.horizontal(|ui| {
            if let Ok(c) = CrosshairCode::parse(&p.code) {
                mini_preview(ui, &c.primary());
            }
            ui.label(RichText::new(&p.name).strong());
            ui.monospace(&p.code);
            if ui.small_button("Edit").clicked() {
                load = Some(i);
            }
            if ui.small_button("Copy").clicked() {
                ui.ctx().copy_text(p.code.clone());
            }
            if ui.small_button("Delete").clicked() {
                delete = Some(i);
            }
        });
    }
    if let Some(i) = load {
        let p = state.settings.crosshairs[i].clone();
        state.crosshair.name = p.name;
        state.crosshair.load(&p.code);
    }
    if let Some(i) = delete {
        state.settings.crosshairs.remove(i);
        state.save_settings(intents);
    }
}

/// Returns true if anything changed.
fn editor(ui: &mut egui::Ui, p: &mut Primary) -> bool {
    let before = *p;
    egui::Grid::new("xhair-edit")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Color");
            egui::ComboBox::from_id_salt("xhair-color")
                .selected_text(COLOR_NAMES.get(p.color_index as usize).copied().unwrap_or("?"))
                .show_ui(ui, |ui| {
                    for (i, name) in COLOR_NAMES.iter().enumerate().take(8) {
                        ui.selectable_value(&mut p.color_index, i as u8, *name);
                    }
                });
            ui.end_row();
            ui.label("Outlines");
            ui.horizontal(|ui| {
                ui.checkbox(&mut p.outlines, "");
                ui.add(egui::Slider::new(&mut p.outline_opacity, 0.0..=1.0).text("opacity"));
            });
            ui.end_row();
            ui.label("Center dot");
            ui.horizontal(|ui| {
                ui.checkbox(&mut p.center_dot, "");
                ui.add(
                    egui::Slider::new(&mut p.dot_thickness, 1.0..=6.0)
                        .step_by(1.0)
                        .text("size"),
                );
            });
            ui.end_row();
            lines_row(ui, "Inner lines", &mut p.inner);
            lines_row(ui, "Outer lines", &mut p.outer);
        });
    *p != before
}

fn lines_row(ui: &mut egui::Ui, label: &str, l: &mut Lines) {
    ui.label(label);
    ui.vertical(|ui| {
        ui.checkbox(&mut l.show, "show");
        ui.add(egui::Slider::new(&mut l.length, 0.0..=20.0).step_by(1.0).text("length"));
        ui.add(
            egui::Slider::new(&mut l.thickness, 0.0..=10.0)
                .step_by(1.0)
                .text("thickness"),
        );
        ui.add(egui::Slider::new(&mut l.offset, 0.0..=40.0).step_by(1.0).text("offset"));
        ui.add(egui::Slider::new(&mut l.opacity, 0.0..=1.0).text("opacity"));
    });
    ui.end_row();
}

fn preview(ui: &mut egui::Ui, p: &Primary) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(180.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, Color32::from_rgb(92, 98, 104));
    draw(&painter, rect.center(), p, 2.0);
}

fn mini_preview(ui: &mut egui::Ui, p: &Primary) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_rgb(92, 98, 104));
    draw(&painter, rect.center(), p, 0.8);
}

fn alpha(c: [u8; 4], a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (a.clamp(0.0, 1.0) * c[3] as f32) as u8)
}

/// Draw the primary crosshair at `scale` screen pixels per game pixel.
pub fn draw(painter: &egui::Painter, c: Pos2, p: &Primary, scale: f32) {
    let mut rects: Vec<(Rect, f32)> = Vec::new();
    for l in [&p.inner, &p.outer] {
        if !l.show || l.length <= 0.0 || l.thickness <= 0.0 {
            continue;
        }
        let (off, len, half) = (l.offset * scale, l.length * scale, l.thickness * scale / 2.0);
        rects.push((
            Rect::from_min_max(Pos2::new(c.x - half, c.y - off - len), Pos2::new(c.x + half, c.y - off)),
            l.opacity,
        ));
        rects.push((
            Rect::from_min_max(Pos2::new(c.x - half, c.y + off), Pos2::new(c.x + half, c.y + off + len)),
            l.opacity,
        ));
        rects.push((
            Rect::from_min_max(Pos2::new(c.x - off - len, c.y - half), Pos2::new(c.x - off, c.y + half)),
            l.opacity,
        ));
        rects.push((
            Rect::from_min_max(Pos2::new(c.x + off, c.y - half), Pos2::new(c.x + off + len, c.y + half)),
            l.opacity,
        ));
    }
    if p.center_dot {
        rects.push((
            Rect::from_center_size(c, Vec2::splat(p.dot_thickness * scale)),
            p.dot_opacity,
        ));
    }
    if p.outlines {
        let grow = p.outline_thickness * scale;
        for (r, _) in &rects {
            painter.rect_filled(r.expand(grow), 0.0, alpha([0, 0, 0, 255], p.outline_opacity));
        }
    }
    for (r, a) in rects {
        painter.rect_filled(r, 0.0, alpha(p.color, a));
    }
}
