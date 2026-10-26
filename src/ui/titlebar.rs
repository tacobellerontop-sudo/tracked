//! Slim custom title bar that replaces the OS one: app mark, drag to move,
//! double-click to maximize, minimize / maximize / close, and edge resizing.
//! Turned off by Settings → "Use the system title bar".

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Rect, ResizeDirection, Sense, Stroke, ViewportCommand};

use super::{icons, theme, theme::pal};
use crate::app::AppState;

pub const HEIGHT: f32 = 34.0;
const BUTTON_W: f32 = 46.0;
/// Width of the invisible resize band along the window edges.
const EDGE: f32 = 5.0;

/// Whether the custom title bar is in use.
pub fn enabled(state: &AppState) -> bool {
    !state.settings.system_title_bar
}

/// Draw the title bar. `sidebar_w` paints the left part in the sidebar colour
/// so the bar lines up with the main layout (0 on full-width screens).
pub fn show(ctx: &egui::Context, state: &AppState, sidebar_w: f32) {
    if !enabled(state) {
        return;
    }
    let p = pal(ctx);
    let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
    edge_resize(ctx, maximized);
    egui::TopBottomPanel::top("titlebar")
        .exact_height(HEIGHT)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(p.bg))
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            if sidebar_w > 0.0 {
                let left = Rect::from_min_size(bar.min, vec2(sidebar_w, bar.height()));
                ui.painter().rect_filled(left, 0.0, p.sidebar);
            }

            // Drag / double-click area first so the buttons drawn after it win.
            let drag = ui.interact(bar, ui.id().with("drag"), Sense::click_and_drag());
            if drag.double_clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            } else if drag.drag_started() {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }

            // App mark.
            let mark = Rect::from_center_size(pos2(bar.left() + 26.0, bar.center().y), vec2(18.0, 18.0));
            icons::logo(ui.painter(), mark, p.accent);
            ui.painter().text(
                pos2(mark.right() + 8.0, bar.center().y),
                Align2::LEFT_CENTER,
                "Tracked",
                theme::semibold(12.5),
                p.text,
            );

            // Window buttons, right to left.
            let mut x = bar.right();
            for button in [Button::Close, Button::Maximize, Button::Minimize] {
                let rect = Rect::from_min_max(pos2(x - BUTTON_W, bar.top()), pos2(x, bar.bottom()));
                x -= BUTTON_W;
                let resp = ui.interact(rect, ui.id().with(button as u8), Sense::click());
                let hot = resp.hovered();
                let (fill, ink) = match (button, hot) {
                    (Button::Close, true) => (Color32::from_rgb(232, 17, 35), Color32::WHITE),
                    (_, true) => (p.selected, p.text),
                    _ => (Color32::TRANSPARENT, p.weak),
                };
                ui.painter().rect_filled(rect, 0.0, fill);
                glyph(ui.painter(), rect.center(), button, maximized, ink);
                if resp.clicked() {
                    ctx.send_viewport_cmd(match button {
                        Button::Close => ViewportCommand::Close,
                        Button::Maximize => ViewportCommand::Maximized(!maximized),
                        Button::Minimize => ViewportCommand::Minimized(true),
                    });
                }
                resp.on_hover_text(match button {
                    Button::Close => "Close",
                    Button::Maximize if maximized => "Restore",
                    Button::Maximize => "Maximize",
                    Button::Minimize => "Minimize",
                });
            }
        });
}

#[derive(Clone, Copy)]
enum Button {
    Minimize,
    Maximize,
    Close,
}

/// Thin 10px line glyphs in the style of the Windows caption buttons.
fn glyph(painter: &egui::Painter, c: egui::Pos2, button: Button, maximized: bool, ink: Color32) {
    let stroke = Stroke::new(1.0_f32, ink);
    let h = 5.0;
    match button {
        Button::Minimize => {
            painter.line_segment([pos2(c.x - h, c.y), pos2(c.x + h, c.y)], stroke);
        }
        Button::Close => {
            painter.line_segment([pos2(c.x - h, c.y - h), pos2(c.x + h, c.y + h)], stroke);
            painter.line_segment([pos2(c.x - h, c.y + h), pos2(c.x + h, c.y - h)], stroke);
        }
        Button::Maximize if maximized => {
            // Restore: two overlapping squares.
            let front = Rect::from_min_max(pos2(c.x - h, c.y - h + 2.0), pos2(c.x + h - 2.0, c.y + h));
            painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment([pos2(c.x - h + 2.0, c.y - h), pos2(c.x + h, c.y - h)], stroke);
            painter.line_segment([pos2(c.x + h, c.y - h), pos2(c.x + h, c.y + h - 2.0)], stroke);
        }
        Button::Maximize => {
            let r = Rect::from_center_size(c, vec2(2.0 * h, 2.0 * h));
            painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Inside);
        }
    }
}

/// Without OS decorations the window has no resize border, so the outer few
/// pixels act as one: resize cursor on hover, native resize on press.
fn edge_resize(ctx: &egui::Context, maximized: bool) {
    if maximized {
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let screen = ctx.screen_rect();
    let (l, r) = (pos.x - screen.left() < EDGE, screen.right() - pos.x < EDGE);
    let (t, b) = (pos.y - screen.top() < EDGE, screen.bottom() - pos.y < EDGE);
    let dir = match (l, r, t, b) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::West,
        (_, true, ..) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return,
    };
    ctx.set_cursor_icon(match dir {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    });
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
    }
}
