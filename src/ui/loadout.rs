//! Loadout screen: your player card and level, XP progress and the daily
//! first-win bonus, what you own, and every equipped skin, buddy and spray.

use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, Layout, Rect, RichText, Sense, Stroke, Ui};

use super::store::{grid, rarity_color};
use super::theme::{self, pal};
use crate::app::AppState;
use crate::ipc::IntentTx;
use crate::shop::{fmt_countdown, GunView, LoadoutView, XP_PER_LEVEL};

pub fn ui(ui: &mut Ui, state: &mut AppState, intents: &IntentTx) {
    let p = pal(ui.ctx());
    if state.shop.loadout.is_none() {
        state.shop.request_loadout(intents);
    }
    let now = chrono::Utc::now().timestamp();
    let view = state.shop.loadout.clone();

    ui.horizontal(|ui| {
        ui.label(RichText::new("Loadout").font(theme::semibold(15.0)).color(p.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if state.shop.loadout_loading {
                ui.spinner();
            } else if theme::pill(ui, "Refresh", false).clicked() {
                state.shop.request_loadout(intents);
            }
            ui.label(
                RichText::new("Change your loadout in VALORANT; this view only reads it.")
                    .size(11.0)
                    .color(p.faint),
            );
        });
    });

    let v = match view {
        None => {
            theme::card(ui, None, |ui| {
                let w = ui.available_width();
                let (r, _) = ui.allocate_exact_size(vec2(w, 150.0), Sense::hover());
                theme::skeleton(ui, r, 14);
            });
            return;
        }
        Some(Err(e)) => {
            theme::card(ui, None, |ui| {
                ui.label(
                    RichText::new("Couldn't load your loadout")
                        .font(theme::semibold(14.0))
                        .color(p.text),
                );
                ui.label(RichText::new(e).color(p.weak));
                ui.add_space(6.0);
                if !state.shop.loadout_loading && theme::pill(ui, "Try again", false).clicked() {
                    state.shop.request_loadout(intents);
                }
            });
            return;
        }
        Some(Ok(v)) => v,
    };
    if v.xp.as_ref().and_then(|x| x.first_win_at).is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
    }

    identity_card(ui, &v, now);
    collection_row(ui, &v);

    theme::card(ui, None, |ui| {
        theme::card_title(ui, "Weapons", None);
        grid(ui, &v.guns, 230.0, 150.0, gun_tile);
    });

    if !v.sprays.is_empty() {
        theme::card(ui, None, |ui| {
            theme::card_title(ui, "Sprays", None);
            grid(ui, &v.sprays, 150.0, 130.0, |ui, r, (name, icon)| {
                ui.painter().rect_filled(r, CornerRadius::same(14), p.well);
                let art = Rect::from_min_max(r.min + vec2(14.0, 12.0), pos2(r.right() - 14.0, r.bottom() - 34.0));
                if icon.is_empty() {
                    super::icons::paint(
                        ui.painter(),
                        Rect::from_center_size(art.center(), vec2(30.0, 30.0)),
                        p.faint,
                        super::icons::Icon::Chat,
                    );
                } else {
                    ui.put(art, egui::Image::new(icon.clone()).fit_to_exact_size(art.size()));
                }
                ui.painter_at(r).text(
                    pos2(r.center().x, r.bottom() - 18.0),
                    Align2::CENTER_CENTER,
                    name,
                    egui::FontId::proportional(12.0),
                    p.text,
                );
            });
        });
    }

    if let Some(xp) = v.xp.as_ref().filter(|x| !x.recent.is_empty()) {
        theme::card(ui, None, |ui| {
            theme::card_title(ui, "Recent XP", None);
            for g in &xp.recent {
                ui.horizontal(|ui| {
                    ui.set_min_height(26.0);
                    ui.label(
                        RichText::new(if g.at > 0 {
                            super::widgets::fmt_date(g.at)
                        } else {
                            "—".into()
                        })
                        .size(12.0)
                        .color(p.weak),
                    );
                    ui.add_space(8.0);
                    for (label, amount) in &g.sources {
                        ui.label(RichText::new(format!("{label} +{amount}")).size(11.5).color(p.faint));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("+{} XP", super::store::format_amount(g.total as i64)))
                                .font(theme::semibold(12.5))
                                .color(p.win),
                        );
                    });
                });
            }
        });
    }
}

/// Player card banner with title, level, XP bar and the first-win timer.
fn identity_card(ui: &mut Ui, v: &LoadoutView, now: i64) {
    let p = pal(ui.ctx());
    theme::card(ui, None, |ui| {
        let w = ui.available_width();
        let (banner, _) = ui.allocate_exact_size(vec2(w, 128.0), Sense::hover());
        theme::image_cover(ui, banner, Some(v.card_wide.as_str()), 452.0 / 128.0, 12);
        if v.card_wide.is_empty() {
            ui.painter().rect_filled(banner, CornerRadius::same(12), p.navy);
            ui.painter()
                .rect_filled(banner, CornerRadius::same(12), p.accent.gamma_multiply(0.25));
        } else {
            // Darken the right side so the title stays readable over the art.
            let shade = Rect::from_min_max(pos2(banner.left() + banner.width() * 0.35, banner.top()), banner.max);
            ui.painter().rect_filled(
                shade,
                CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: 12,
                    se: 12,
                },
                Color32::from_black_alpha(90),
            );
        }
        let x = banner.right() - 22.0;
        let title = if v.title.is_empty() {
            "No title"
        } else {
            v.title.as_str()
        };
        ui.painter().text(
            pos2(x, banner.top() + 34.0),
            Align2::RIGHT_CENTER,
            title,
            theme::semibold(18.0),
            Color32::WHITE,
        );
        if !v.card_name.is_empty() {
            ui.painter().text(
                pos2(x, banner.top() + 58.0),
                Align2::RIGHT_CENTER,
                &v.card_name,
                egui::FontId::proportional(11.5),
                Color32::from_white_alpha(190),
            );
        }
        let level = v.xp.as_ref().map(|x| x.level).filter(|l| *l > 0).unwrap_or(v.level);
        let chip = Rect::from_min_size(pos2(x - 120.0, banner.bottom() - 42.0), vec2(120.0, 28.0));
        ui.painter()
            .rect_filled(chip, CornerRadius::same(14), Color32::from_black_alpha(150));
        ui.painter().text(
            chip.center(),
            Align2::CENTER_CENTER,
            format!("Level {level}"),
            theme::semibold(13.0),
            Color32::WHITE,
        );

        ui.add_space(12.0);
        if let Some(xp) = &v.xp {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Next level").size(12.0).color(p.weak));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} / {} XP",
                            super::store::format_amount(xp.xp as i64),
                            super::store::format_amount(XP_PER_LEVEL as i64)
                        ))
                        .size(12.0)
                        .color(p.text),
                    );
                });
            });
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
            ui.painter().rect_filled(bar, CornerRadius::same(4), p.well);
            let frac = (xp.xp as f32 / XP_PER_LEVEL as f32).clamp(0.0, 1.0);
            let frac = theme::glide(ui.ctx(), egui::Id::new("xp-bar"), frac, 0.5);
            let fill = Rect::from_min_size(bar.min, vec2(bar.width() * frac, bar.height()));
            ui.painter().rect_filled(fill, CornerRadius::same(4), p.accent);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
                let (text, color) = match xp.first_win_at {
                    None => ("Daily first-win bonus is ready (+1,000 XP)".to_string(), p.win),
                    Some(t) => (
                        format!("Daily first-win bonus back in {}", fmt_countdown(t - now)),
                        p.warn,
                    ),
                };
                ui.painter().circle_filled(dot.center(), 4.0, color);
                ui.label(RichText::new(text).size(12.0).color(p.text));
            });
        }
        if v.level_hidden || v.incognito {
            let mut hidden = Vec::new();
            if v.level_hidden {
                hidden.push("your level is hidden from other players");
            }
            if v.incognito {
                hidden.push("streamer mode is on");
            }
            ui.label(
                RichText::new(format!("Note: {}.", hidden.join(", ")))
                    .size(11.5)
                    .color(p.faint),
            );
        }
    });
}

fn collection_row(ui: &mut Ui, v: &LoadoutView) {
    let c = &v.collection;
    let stats = [
        ("Skins", c.skins),
        ("Gun buddies", c.buddies),
        ("Sprays", c.sprays),
        ("Player cards", c.cards),
        ("Titles", c.titles),
    ];
    let stats: Vec<_> = stats.into_iter().filter_map(|(l, n)| n.map(|n| (l, n))).collect();
    if stats.is_empty() {
        return;
    }
    let p = pal(ui.ctx());
    theme::card(ui, None, |ui| {
        theme::card_title(ui, "Collection", None);
        grid(ui, &stats, 120.0, 58.0, |ui, r, (label, n)| {
            ui.painter().rect_filled(r, CornerRadius::same(12), p.well);
            ui.painter().text(
                r.left_center() + vec2(14.0, -9.0),
                Align2::LEFT_CENTER,
                super::store::format_amount(*n as i64),
                theme::semibold(18.0),
                p.text,
            );
            ui.painter().text(
                r.left_center() + vec2(14.0, 13.0),
                Align2::LEFT_CENTER,
                *label,
                egui::FontId::proportional(11.5),
                p.weak,
            );
        });
    });
}

fn gun_tile(ui: &mut Ui, rect: Rect, g: &GunView) {
    let p = pal(ui.ctx());
    let color = rarity_color(g.rarity.as_ref(), &p);
    ui.painter().rect_filled(rect, CornerRadius::same(14), p.well);
    if g.rarity.is_some() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(14), color.gamma_multiply(0.10));
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(14),
            Stroke::new(1.0_f32, color.gamma_multiply(0.35)),
            egui::StrokeKind::Inside,
        );
    }
    let painter = ui.painter_at(rect);
    painter.text(
        rect.left_top() + vec2(14.0, 16.0),
        Align2::LEFT_CENTER,
        g.weapon.to_uppercase(),
        theme::semibold(10.5),
        p.weak,
    );
    let art = Rect::from_min_max(
        rect.min + vec2(18.0, 28.0),
        pos2(rect.right() - 18.0, rect.bottom() - 48.0),
    );
    if g.image.is_empty() {
        super::icons::paint(
            &painter,
            Rect::from_center_size(art.center(), vec2(30.0, 30.0)),
            color.gamma_multiply(0.6),
            super::icons::Icon::Swords,
        );
    } else {
        ui.put(art, egui::Image::new(g.image.clone()).fit_to_exact_size(art.size()));
    }
    painter.text(
        pos2(rect.left() + 14.0, rect.bottom() - 32.0),
        Align2::LEFT_CENTER,
        &g.skin,
        theme::semibold(12.5),
        p.text,
    );
    let sub = match (&g.variant, &g.rarity) {
        (Some(v), _) => v.clone(),
        (None, Some(r)) => r.name.clone(),
        (None, None) => "Default".into(),
    };
    painter.text(
        pos2(rect.left() + 14.0, rect.bottom() - 14.0),
        Align2::LEFT_CENTER,
        sub,
        egui::FontId::proportional(11.0),
        if g.rarity.is_some() { color } else { p.faint },
    );
    if let Some((name, icon)) = &g.buddy {
        let b = Rect::from_center_size(pos2(rect.right() - 22.0, rect.top() + 18.0), vec2(22.0, 22.0));
        if icon.is_empty() {
            super::icons::paint(&painter, b, p.weak, super::icons::Icon::Medal);
        } else {
            egui::Image::new(icon.clone()).paint_at(ui, b);
        }
        ui.interact(b, ui.id().with(("buddy", &g.weapon_id)), Sense::hover())
            .on_hover_text(name);
    }
}
