//! Look & feel: palettes for light (default) and dark, the egui style, and
//! small building blocks (cards, pills, buttons, deltas) used by every screen.

use egui::{Color32, CornerRadius, Frame, Margin, Response, RichText, Sense, Shadow, Stroke, Ui, Vec2};

use crate::config::Theme;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    /// Window background.
    pub bg: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    /// Inputs, tracks, empty image slots.
    pub well: Color32,
    pub stroke: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub win: Color32,
    pub loss: Color32,
    pub warn: Color32,
    /// Primary button.
    pub navy: Color32,
    pub on_navy: Color32,
    /// Selected nav item / selected pill.
    pub selected: Color32,
    pub grid: Color32,
}

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(238, 236, 235),
    sidebar: Color32::from_rgb(243, 241, 240),
    card: Color32::from_rgb(248, 247, 246),
    well: Color32::from_rgb(236, 233, 232),
    stroke: Color32::from_rgb(228, 224, 222),
    text: Color32::from_rgb(38, 41, 48),
    weak: Color32::from_rgb(128, 131, 139),
    faint: Color32::from_rgb(178, 179, 184),
    accent: Color32::from_rgb(226, 98, 108),
    win: Color32::from_rgb(38, 166, 136),
    loss: Color32::from_rgb(222, 88, 92),
    warn: Color32::from_rgb(214, 160, 52),
    navy: Color32::from_rgb(31, 37, 50),
    on_navy: Color32::from_rgb(244, 244, 246),
    selected: Color32::from_rgb(229, 225, 224),
    grid: Color32::from_rgb(229, 226, 225),
};

pub const DARK: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(19, 21, 26),
    sidebar: Color32::from_rgb(23, 25, 31),
    card: Color32::from_rgb(29, 32, 39),
    well: Color32::from_rgb(37, 41, 50),
    stroke: Color32::from_rgb(43, 47, 57),
    text: Color32::from_rgb(230, 231, 235),
    weak: Color32::from_rgb(140, 144, 154),
    faint: Color32::from_rgb(92, 96, 106),
    accent: Color32::from_rgb(236, 104, 114),
    win: Color32::from_rgb(52, 190, 156),
    loss: Color32::from_rgb(236, 98, 100),
    warn: Color32::from_rgb(226, 176, 72),
    navy: Color32::from_rgb(232, 233, 237),
    on_navy: Color32::from_rgb(25, 28, 35),
    selected: Color32::from_rgb(40, 44, 54),
    grid: Color32::from_rgb(40, 44, 53),
};

fn palette_id() -> egui::Id {
    egui::Id::new("ascend-palette")
}

/// The active palette (set by [`apply`]).
pub fn pal(ctx: &egui::Context) -> Palette {
    ctx.data(|d| d.get_temp::<Palette>(palette_id())).unwrap_or(LIGHT)
}

/// Install the palette and egui style for `theme`.
pub fn apply(ctx: &egui::Context, theme: Theme) {
    let p = match theme {
        Theme::Light => LIGHT,
        Theme::Dark => DARK,
    };
    ctx.data_mut(|d| d.insert_temp(palette_id(), p));

    let mut v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.extreme_bg_color = p.well;
    v.faint_bg_color = if p.dark {
        Color32::from_rgb(33, 36, 44)
    } else {
        Color32::from_rgb(242, 240, 239)
    };
    v.window_stroke = Stroke::new(1.0_f32, p.stroke);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.stroke);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.noninteractive.bg_fill = p.card;
    for (w, fill) in [
        (&mut v.widgets.inactive, p.well),
        (&mut v.widgets.hovered, p.selected),
        (&mut v.widgets.active, p.selected),
        (&mut v.widgets.open, p.selected),
    ] {
        w.weak_bg_fill = fill;
        w.bg_fill = fill;
        w.corner_radius = CornerRadius::same(8);
        w.fg_stroke = Stroke::new(1.0_f32, p.text);
        w.bg_stroke = Stroke::NONE;
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.stroke);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.slider_trailing_fill = true;
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        use egui::{FontFamily, FontId, TextStyle};
        s.text_styles = [
            (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.spacing.interact_size.y = 24.0;
    });
}

/// The standard card container.
pub fn card_frame(p: &Palette) -> Frame {
    Frame::NONE
        .fill(p.card)
        .stroke(Stroke::new(1.0_f32, p.stroke))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .shadow(Shadow {
            offset: [0, 2],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 40 } else { 10 }),
        })
}

/// A card filling the available width, with optional fixed height.
pub fn card<R>(ui: &mut Ui, height: Option<f32>, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = pal(ui.ctx());
    card_frame(&p)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
            if let Some(h) = height {
                ui.set_min_height(h - 36.0);
            }
            add(ui)
        })
        .inner
}

/// Card title with an optional right-aligned link ("View all →").
/// Returns true when the link was clicked.
pub fn card_title(ui: &mut Ui, title: &str, link: Option<&str>) -> bool {
    let p = pal(ui.ctx());
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(15.0).color(p.text).strong());
        if let Some(link) = link {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = link_button(ui, link).clicked();
            });
        }
    });
    ui.add_space(8.0);
    clicked
}

/// Small text link with a trailing arrow.
pub fn link_button(ui: &mut Ui, text: &str) -> Response {
    let p = pal(ui.ctx());
    let r = ui.add(egui::Button::new(RichText::new(text).size(11.5).color(p.weak)).frame(false));
    let arrow = egui::Rect::from_min_size(r.rect.right_center() + egui::vec2(-2.0, -4.0), egui::vec2(8.0, 8.0));
    super::icons::paint(ui.painter(), arrow, p.weak, super::icons::Icon::ArrowRight);
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Rounded toggle pill ("7D", "30D"…).
pub fn pill(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let p = pal(ui.ctx());
    let text = RichText::new(label)
        .size(11.5)
        .color(if selected { p.on_navy } else { p.weak });
    let fill = if selected { p.navy } else { Color32::TRANSPARENT };
    ui.add(
        egui::Button::new(text)
            .fill(fill)
            .corner_radius(CornerRadius::same(12))
            .min_size(Vec2::new(40.0, 24.0)),
    )
}

/// Full-width primary button with a trailing arrow.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let p = pal(ui.ctx());
    let size = Vec2::new(ui.available_width(), 36.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.hovered() {
        p.navy.gamma_multiply(0.9)
    } else {
        p.navy
    };
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(13.0),
        p.on_navy,
    );
    let arrow = egui::Rect::from_center_size(rect.right_center() - egui::vec2(22.0, 0.0), egui::vec2(10.0, 10.0));
    super::icons::paint(ui.painter(), arrow, p.on_navy, super::icons::Icon::ArrowRight);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// "▲ 3.1%" in green or "▼ 2.0%" in red; `suffix` follows ("vs lobby").
pub fn delta(ui: &mut Ui, pct: f32, suffix: &str) {
    delta_with_unit(ui, pct, "%", suffix)
}

/// Like [`delta`] with a custom unit ("%", " pts").
pub fn delta_with_unit(ui: &mut Ui, pct: f32, unit: &str, suffix: &str) {
    let p = pal(ui.ctx());
    let up = pct >= 0.0;
    let color = if up { p.win } else { p.loss };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, 12.0), Sense::hover());
        let c = rect.center();
        let pts = if up {
            vec![
                c + egui::vec2(0.0, -4.0),
                c + egui::vec2(4.0, 2.5),
                c + egui::vec2(-4.0, 2.5),
            ]
        } else {
            vec![
                c + egui::vec2(0.0, 4.0),
                c + egui::vec2(4.0, -2.5),
                c + egui::vec2(-4.0, -2.5),
            ]
        };
        ui.painter().add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
        ui.label(RichText::new(format!("{:.1}{unit}", pct.abs())).size(11.5).color(color));
        if !suffix.is_empty() {
            ui.label(RichText::new(suffix).size(11.0).color(p.faint));
        }
    });
}

/// Paint an image (or a placeholder) into `rect`, cropped to fill it.
/// `aspect` is the source image's width/height, if known.
pub fn image_cover(ui: &Ui, rect: egui::Rect, uri: Option<&str>, aspect: f32, radius: impl Into<CornerRadius>) {
    let radius: CornerRadius = radius.into();
    let p = pal(ui.ctx());
    match uri.filter(|u| !u.is_empty()) {
        Some(u) => {
            let target = rect.width() / rect.height();
            let uv = if aspect > target {
                let w = target / aspect;
                egui::Rect::from_min_max(egui::pos2(0.5 - w / 2.0, 0.0), egui::pos2(0.5 + w / 2.0, 1.0))
            } else {
                let h = aspect / target;
                egui::Rect::from_min_max(egui::pos2(0.0, 0.5 - h / 2.0), egui::pos2(1.0, 0.5 + h / 2.0))
            };
            ui.painter().rect_filled(rect, radius, p.well);
            egui::Image::new(u.to_string())
                .uv(uv)
                .corner_radius(radius)
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, radius, p.well);
        }
    }
}

/// A circular avatar image (or initials on a tinted disc).
pub fn avatar(ui: &mut Ui, uri: Option<&str>, initials: &str, size: f32) -> Response {
    let p = pal(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let r = (size / 2.0) as u8;
    match uri.filter(|u| !u.is_empty()) {
        Some(_) => image_cover(ui, rect, uri, 1.0, r),
        None => {
            ui.painter()
                .circle_filled(rect.center(), size / 2.0, p.accent.gamma_multiply(0.18));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                initials,
                egui::FontId::proportional(size * 0.38),
                p.accent,
            );
        }
    }
    resp
}
