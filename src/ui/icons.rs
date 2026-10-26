//! Icons (Phosphor, MIT) behind a small enum, plus the painted app logo.

use egui::{vec2, Color32, Painter, Rect, Shape, Stroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Home,
    Live,
    Matches,
    Agents,
    Search,
    Crosshair,
    Social,
    Settings,
    Connection,
    Sun,
    Moon,
    ArrowRight,
    Target,
    Swords,
    Medal,
    ChartUp,
    ChartDown,
    Gamepad,
    Send,
    UserPlus,
    Users,
    Chat,
    Play,
    Pause,
    Prev,
    Next,
    PrevRound,
    NextRound,
    Map,
    Back,
}

impl Icon {
    /// The Phosphor glyph for this icon.
    pub fn glyph(self) -> &'static str {
        use egui_phosphor::regular as ph;
        match self {
            Icon::Home => ph::HOUSE,
            Icon::Live => ph::BROADCAST,
            Icon::Matches => ph::CLOCK_COUNTER_CLOCKWISE,
            Icon::Agents => ph::USERS_THREE,
            Icon::Search => ph::MAGNIFYING_GLASS,
            Icon::Crosshair => ph::CROSSHAIR,
            Icon::Social => ph::CHATS_CIRCLE,
            Icon::Settings => ph::GEAR_SIX,
            Icon::Connection => ph::PLUGS_CONNECTED,
            Icon::Sun => ph::SUN,
            Icon::Moon => ph::MOON,
            Icon::ArrowRight => ph::ARROW_RIGHT,
            Icon::Target => ph::TARGET,
            Icon::Swords => ph::SWORD,
            Icon::Medal => ph::MEDAL,
            Icon::ChartUp => ph::TREND_UP,
            Icon::ChartDown => ph::TREND_DOWN,
            Icon::Gamepad => ph::GAME_CONTROLLER,
            Icon::Send => ph::PAPER_PLANE_RIGHT,
            Icon::UserPlus => ph::USER_PLUS,
            Icon::Users => ph::USERS,
            Icon::Chat => ph::CHAT_CIRCLE_DOTS,
            Icon::Play => ph::PLAY,
            Icon::Pause => ph::PAUSE,
            Icon::Prev => ph::CARET_LEFT,
            Icon::Next => ph::CARET_RIGHT,
            Icon::PrevRound => ph::SKIP_BACK,
            Icon::NextRound => ph::SKIP_FORWARD,
            Icon::Map => ph::MAP_TRIFOLD,
            Icon::Back => ph::ARROW_LEFT,
        }
    }
}

/// Draw `icon` centered in `rect`.
pub fn paint(painter: &Painter, rect: Rect, color: Color32, icon: Icon) {
    let size = rect.width().min(rect.height());
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon.glyph(),
        super::theme::icon_font(size),
        color,
    );
}

/// Allocate a square and draw an icon in it.
pub fn icon(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    paint(ui.painter(), rect, color, icon);
    resp
}

/// The app mark: an upward chevron over a bar (ascend), drawn in `color`.
pub fn logo(painter: &Painter, rect: Rect, color: Color32) {
    let s = rect.width().min(rect.height()) / 20.0;
    let o = rect.center() - vec2(10.0, 10.0) * s;
    let p = |x: f32, y: f32| o + vec2(x, y) * s;
    painter.add(Shape::convex_polygon(
        vec![p(10.0, 2.0), p(18.5, 12.0), p(14.5, 12.0), p(10.0, 6.8)],
        color,
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        vec![p(10.0, 2.0), p(10.0, 6.8), p(5.5, 12.0), p(1.5, 12.0)],
        color,
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        vec![p(4.0, 15.0), p(16.0, 15.0), p(14.5, 17.5), p(5.5, 17.5)],
        color.gamma_multiply(0.6),
        Stroke::NONE,
    ));
}
