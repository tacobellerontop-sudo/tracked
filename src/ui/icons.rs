//! Small line icons drawn with the painter, so they're crisp at any scale and
//! don't depend on font glyph coverage.

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

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
}

/// Draw `icon` inside `rect` (drawn on a 20×20 grid, scaled to fit).
pub fn paint(painter: &Painter, rect: Rect, color: Color32, icon: Icon) {
    let s = rect.width().min(rect.height()) / 20.0;
    let o = rect.center() - vec2(10.0, 10.0) * s;
    let p = |x: f32, y: f32| -> Pos2 { o + vec2(x, y) * s };
    let st = Stroke::new(1.6 * s.max(0.75), color);
    let line = |pts: Vec<Pos2>| Shape::line(pts, st);
    let shapes: Vec<Shape> = match icon {
        Icon::Home => vec![
            line(vec![p(3.0, 9.5), p(10.0, 3.5), p(17.0, 9.5)]),
            line(vec![p(5.0, 8.0), p(5.0, 16.5), p(15.0, 16.5), p(15.0, 8.0)]),
            line(vec![p(8.5, 16.5), p(8.5, 12.0), p(11.5, 12.0), p(11.5, 16.5)]),
        ],
        Icon::Live => vec![
            Shape::circle_filled(p(10.0, 10.0), 2.0 * s, color),
            Shape::line(arc(p(10.0, 10.0), 5.0 * s, -0.8, 0.8), st),
            Shape::line(arc(p(10.0, 10.0), 5.0 * s, 2.34, 3.94), st),
            Shape::line(arc(p(10.0, 10.0), 8.0 * s, -0.8, 0.8), st),
            Shape::line(arc(p(10.0, 10.0), 8.0 * s, 2.34, 3.94), st),
        ],
        Icon::Matches => vec![
            Shape::circle_stroke(p(10.0, 10.0), 7.0 * s, st),
            line(vec![p(10.0, 5.5), p(10.0, 10.0), p(13.0, 12.0)]),
        ],
        Icon::Agents => vec![
            Shape::circle_stroke(p(10.0, 7.0), 3.2 * s, st),
            Shape::line(
                arc(p(10.0, 17.5), 6.0 * s, std::f32::consts::PI, std::f32::consts::TAU),
                st,
            ),
        ],
        Icon::Search => vec![
            Shape::circle_stroke(p(8.5, 8.5), 5.0 * s, st),
            line(vec![p(12.3, 12.3), p(16.5, 16.5)]),
        ],
        Icon::Crosshair => vec![
            Shape::circle_stroke(p(10.0, 10.0), 6.0 * s, st),
            line(vec![p(10.0, 2.0), p(10.0, 6.5)]),
            line(vec![p(10.0, 13.5), p(10.0, 18.0)]),
            line(vec![p(2.0, 10.0), p(6.5, 10.0)]),
            line(vec![p(13.5, 10.0), p(18.0, 10.0)]),
        ],
        Icon::Social => vec![line(vec![
            p(4.0, 4.5),
            p(16.0, 4.5),
            p(16.0, 12.5),
            p(9.0, 12.5),
            p(5.5, 15.5),
            p(5.5, 12.5),
            p(4.0, 12.5),
            p(4.0, 4.5),
        ])],
        Icon::Settings => {
            let mut v = vec![
                Shape::circle_stroke(p(10.0, 10.0), 2.6 * s, st),
                Shape::circle_stroke(p(10.0, 10.0), 5.6 * s, st),
            ];
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (sn, cs) = a.sin_cos();
                v.push(line(vec![
                    p(10.0 + cs * 5.6, 10.0 + sn * 5.6),
                    p(10.0 + cs * 7.8, 10.0 + sn * 7.8),
                ]));
            }
            v
        }
        Icon::Connection => vec![
            line(vec![p(7.0, 3.0), p(7.0, 6.5)]),
            line(vec![p(13.0, 3.0), p(13.0, 6.5)]),
            line(vec![
                p(4.5, 6.5),
                p(15.5, 6.5),
                p(15.5, 9.5),
                p(12.5, 13.5),
                p(7.5, 13.5),
                p(4.5, 9.5),
                p(4.5, 6.5),
            ]),
            line(vec![p(10.0, 13.5), p(10.0, 17.5)]),
        ],
        Icon::Sun => {
            let mut v = vec![Shape::circle_stroke(p(10.0, 10.0), 3.5 * s, st)];
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (sn, cs) = a.sin_cos();
                v.push(line(vec![
                    p(10.0 + cs * 5.8, 10.0 + sn * 5.8),
                    p(10.0 + cs * 7.8, 10.0 + sn * 7.8),
                ]));
            }
            v
        }
        Icon::Moon => vec![
            Shape::line(arc(p(10.0, 10.0), 6.5 * s, 0.9, 5.4), st),
            Shape::line(arc(p(13.0, 8.0), 5.0 * s, 1.8, 4.6), st),
        ],
        Icon::ArrowRight => vec![
            line(vec![p(3.0, 10.0), p(17.0, 10.0)]),
            line(vec![p(11.5, 4.5), p(17.0, 10.0), p(11.5, 15.5)]),
        ],
        Icon::Target => vec![
            Shape::circle_stroke(p(10.0, 10.0), 7.0 * s, st),
            Shape::circle_stroke(p(10.0, 10.0), 3.5 * s, st),
            Shape::circle_filled(p(10.0, 10.0), 1.2 * s, color),
        ],
        Icon::Swords => vec![
            line(vec![p(4.0, 4.0), p(14.5, 14.5)]),
            line(vec![p(16.0, 4.0), p(5.5, 14.5)]),
            line(vec![p(12.5, 16.5), p(16.5, 12.5)]),
            line(vec![p(3.5, 12.5), p(7.5, 16.5)]),
        ],
        Icon::Medal => vec![
            Shape::circle_stroke(p(10.0, 12.0), 4.5 * s, st),
            line(vec![p(6.5, 3.0), p(9.0, 7.8)]),
            line(vec![p(13.5, 3.0), p(11.0, 7.8)]),
        ],
        Icon::ChartUp => vec![
            line(vec![p(3.0, 15.0), p(8.0, 10.0), p(11.0, 12.5), p(17.0, 5.5)]),
            line(vec![p(13.0, 5.5), p(17.0, 5.5), p(17.0, 9.5)]),
        ],
        Icon::ChartDown => vec![
            line(vec![p(3.0, 5.5), p(8.0, 10.5), p(11.0, 8.0), p(17.0, 15.0)]),
            line(vec![p(13.0, 15.0), p(17.0, 15.0), p(17.0, 11.0)]),
        ],
        Icon::Gamepad => vec![
            line(vec![
                p(5.0, 6.0),
                p(15.0, 6.0),
                p(18.0, 14.0),
                p(15.0, 15.5),
                p(12.5, 12.0),
                p(7.5, 12.0),
                p(5.0, 15.5),
                p(2.0, 14.0),
                p(5.0, 6.0),
            ]),
            line(vec![p(6.0, 8.5), p(6.0, 10.5)]),
            line(vec![p(5.0, 9.5), p(7.0, 9.5)]),
            Shape::circle_filled(p(13.8, 9.0), 0.9 * s, color),
        ],
    };
    painter.extend(shapes);
}

fn arc(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let n = 16;
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
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
