//! Look & feel: palettes for light (default) and dark, the egui style, and
//! small building blocks (cards, pills, buttons, deltas) used by every screen.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Response, RichText, Sense,
    Shadow, Stroke, Ui, Vec2,
};

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

/// Inter (SIL Open Font License, see assets/fonts) for text, Phosphor for
/// icons, egui's defaults as fallbacks (emoji, CJK, monospace).
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions(&[]));
    // Scripts Inter doesn't cover (Chinese, Japanese, Korean, Thai, Arabic…)
    // come from fonts the OS already has. They're large, so they load in the
    // background and the fonts are swapped in when ready.
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name("fonts".into())
        .spawn(move || {
            let extra = load_script_fonts();
            if !extra.is_empty() {
                tracing::info!(fonts = ?extra.iter().map(|f| &f.0).collect::<Vec<_>>(), "script fallback fonts loaded");
                ctx.set_fonts(font_definitions(&extra));
                ctx.request_repaint();
            }
        })
        .ok();
}

/// System fonts for scripts Inter lacks, in fallback order (first found per
/// script). Windows paths first; the Linux ones are for development.
const SCRIPT_FONTS: &[(&str, &[&str])] = &[
    // Arabic, Hebrew, Armenian, Georgian and more.
    (
        "segoe-ui",
        &[
            r"C:\Windows\Fonts\segoeui.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
        ],
    ),
    // Chinese (YaHei also has traditional characters; on Linux these fonts
    // cover kana and Hangul too).
    (
        "cjk-sc",
        &[
            r"C:\Windows\Fonts\msyh.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        ],
    ),
    // Japanese kana.
    (
        "japanese",
        &[
            r"C:\Windows\Fonts\YuGothR.ttc",
            r"C:\Windows\Fonts\meiryo.ttc",
            r"C:\Windows\Fonts\msgothic.ttc",
        ],
    ),
    // Korean Hangul.
    ("korean", &[r"C:\Windows\Fonts\malgun.ttf"]),
    // Thai, Lao, Khmer.
    (
        "thai",
        &[
            r"C:\Windows\Fonts\LeelawUI.ttf",
            r"C:\Windows\Fonts\leelawad.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf",
        ],
    ),
    // Devanagari and other Indian scripts.
    (
        "indic",
        &[r"C:\Windows\Fonts\Nirmala.ttf", r"C:\Windows\Fonts\Nirmala.ttc"],
    ),
];

/// Read the first available file for each script. Windows paths follow
/// the actual Windows folder (`%WINDIR%`), which isn't always `C:\Windows`.
pub fn load_script_fonts() -> Vec<(String, Arc<FontData>)> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    SCRIPT_FONTS
        .iter()
        .filter_map(|(key, paths)| {
            let bytes = paths.iter().find_map(|p| {
                let path = match p.strip_prefix(r"C:\Windows") {
                    Some(rest) => format!("{windir}{rest}"),
                    None => p.to_string(),
                };
                std::fs::read(path).ok()
            })?;
            Some((key.to_string(), Arc::new(FontData::from_owned(bytes))))
        })
        .collect()
}

/// Inter (three weights), Phosphor icons, egui's defaults, and `extra`
/// script fonts appended as fallbacks to every text family.
pub fn font_definitions(extra: &[(String, Arc<FontData>)]) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, key: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(key.to_string(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        &mut fonts,
        "inter",
        include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    );
    add(
        &mut fonts,
        "inter-medium",
        include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    );
    add(
        &mut fonts,
        "inter-semibold",
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    );
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // Icons get their own family: Inter has private-use glyphs of its own and
    // Phosphor has blank Latin ones, so neither can shadow the other.
    fonts
        .families
        .insert(FontFamily::Name("icons".into()), vec!["phosphor".into()]);
    for (key, data) in extra {
        fonts.font_data.insert(key.clone(), data.clone());
    }
    let mut fallbacks: Vec<String> = fonts.families[&FontFamily::Proportional].clone();
    fallbacks.extend(extra.iter().map(|(k, _)| k.clone()));
    for (family, key) in [
        (FontFamily::Proportional, "inter"),
        (FontFamily::Name("medium".into()), "inter-medium"),
        (FontFamily::Name("semibold".into()), "inter-semibold"),
    ] {
        let mut list = vec![key.to_string()];
        list.extend(fallbacks.iter().cloned());
        fonts.families.insert(family, list);
    }
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.extend(extra.iter().map(|(k, _)| k.clone()));
    }
    fonts
}

/// Font for Phosphor icon glyphs.
pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("icons".into()))
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

/// Human description of the active renderer, e.g.
/// "GPU · NVIDIA GeForce RTX 3060 (Dx12)".
pub fn describe_renderer(cc: &eframe::CreationContext<'_>) -> String {
    if let Some(rs) = &cc.wgpu_render_state {
        let info = rs.adapter.get_info();
        let kind = match info.device_type {
            eframe::wgpu::DeviceType::Cpu => "Software",
            _ => "GPU",
        };
        return format!("{kind} · {} ({:?})", info.name, info.backend);
    }
    if let Some(gl) = &cc.gl {
        use eframe::glow::HasContext;
        // SAFETY: querying a string from a live GL context on the UI thread.
        let name = unsafe { gl.get_parameter_string(eframe::glow::RENDERER) };
        let software = ["llvmpipe", "softpipe", "swiftshader", "gdi generic", "basic render"]
            .iter()
            .any(|w| name.to_lowercase().contains(w));
        let kind = if software { "Software" } else { "GPU" };
        return format!("{kind} · {name} (OpenGL)");
    }
    "Unknown renderer".into()
}

fn anim_id() -> egui::Id {
    egui::Id::new("ascend-animations")
}

/// Turn interface animations on or off (Settings).
pub fn set_animations(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(anim_id(), on));
}

pub fn animations(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(anim_id())).unwrap_or(true)
}

/// Ease-out cubic.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// 0→1 progress of an "intro" animation keyed by `id` (restarts when `key`
/// changes, e.g. a chart's range). Keeps repainting until finished.
pub fn intro(ctx: &egui::Context, id: egui::Id, key: u64, seconds: f32) -> f32 {
    if !animations(ctx) {
        return 1.0;
    }
    let now = ctx.input(|i| i.time);
    let start = ctx.data_mut(|d| {
        let slot = d.get_temp_mut_or_insert_with(id, || (key, now));
        if slot.0 != key {
            *slot = (key, now);
        }
        slot.1
    });
    let t = ((now - start) as f32 / seconds).clamp(0.0, 1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    ease(t)
}

/// Smoothly animate towards `target` (for sliding highlights, hovers).
pub fn glide(ctx: &egui::Context, id: egui::Id, target: f32, seconds: f32) -> f32 {
    if !animations(ctx) {
        return target;
    }
    ctx.animate_value_with_time(id, target, seconds)
}

/// A loading placeholder with a moving sheen.
pub fn skeleton(ui: &Ui, rect: egui::Rect, radius: u8) {
    let p = pal(ui.ctx());
    ui.painter().rect_filled(rect, CornerRadius::same(radius), p.well);
    if !animations(ui.ctx()) {
        return;
    }
    let t = (ui.input(|i| i.time) * 0.8).fract() as f32;
    let band = rect.width() * 0.35;
    let x = rect.left() - band + (rect.width() + band * 2.0) * t;
    let sheen = egui::Rect::from_min_max(
        egui::pos2(x.max(rect.left()), rect.top()),
        egui::pos2((x + band).min(rect.right()), rect.bottom()),
    );
    if sheen.width() > 0.0 {
        let c = if p.dark {
            Color32::from_white_alpha(10)
        } else {
            Color32::from_white_alpha(90)
        };
        ui.painter().rect_filled(sheen, CornerRadius::same(radius), c);
    }
    ui.ctx().request_repaint();
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
        use egui::TextStyle;
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(20.0, FontFamily::Name("semibold".into())),
            ),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(12.5, FontFamily::Name("medium".into()))),
            (TextStyle::Small, FontId::new(10.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.animation_time = 0.18;
    });
}

/// The standard card container.
pub fn card_frame(p: &Palette) -> Frame {
    Frame::NONE
        .fill(p.card)
        .stroke(Stroke::new(1.0_f32, p.stroke))
        .corner_radius(CornerRadius::same(18))
        .inner_margin(Margin::same(20))
        .shadow(Shadow {
            offset: [0, 6],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 60 } else { 12 }),
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
        ui.label(RichText::new(title).font(semibold(15.0)).color(p.text));
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
