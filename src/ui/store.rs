//! Store screen: wallet, daily offers, night market, featured bundles and
//! the Kingdom Credits accessory store. Read-only: nothing here buys anything.

use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, Layout, Rect, RichText, Sense, Stroke, Ui};

use super::theme::{self, pal, Palette};
use crate::app::AppState;
use crate::ipc::IntentTx;
use crate::shop::{currency_short, fmt_countdown, Rarity, ShopItem, StoreView};

pub fn ui(ui: &mut Ui, state: &mut AppState, intents: &IntentTx) {
    let p = pal(ui.ctx());
    if state.shop.store.is_none() {
        state.shop.request_store(intents);
    }
    let now = chrono::Utc::now().timestamp();
    let view = state.shop.store.clone();

    // Header: title, wallet, refresh.
    ui.horizontal(|ui| {
        ui.label(RichText::new("Store").font(theme::semibold(15.0)).color(p.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if state.shop.store_loading {
                ui.spinner();
            } else if theme::pill(ui, "Refresh", false).clicked() {
                state.shop.request_store(intents);
            }
            if let Some(Ok(v)) = &view {
                ui.add_space(6.0);
                for b in v.wallet.iter().rev() {
                    balance_chip(ui, &b.icon, &b.name, b.amount, &b.currency);
                }
            }
        });
    });

    let v = match view {
        None => {
            theme::card(ui, None, |ui| loading_tiles(ui, 4, 190.0));
            return;
        }
        Some(Err(e)) => {
            theme::card(ui, None, |ui| {
                ui.label(
                    RichText::new("Couldn't load your store")
                        .font(theme::semibold(14.0))
                        .color(p.text),
                );
                ui.label(RichText::new(e).color(p.weak));
                ui.add_space(6.0);
                if !state.shop.store_loading && theme::pill(ui, "Try again", false).clicked() {
                    state.shop.request_store(intents);
                }
            });
            return;
        }
        Some(Ok(v)) => v,
    };

    // Reload when the daily store (or anything else) rolls over.
    if let Some(reset) = v.next_reset() {
        if now >= reset {
            state.shop.request_store(intents);
        }
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));

    theme::card(ui, None, |ui| {
        section_title(ui, "Daily offers", v.daily_ends_at, now, "Resets in");
        if v.daily.is_empty() {
            ui.weak("No daily offers right now.");
        } else {
            tile_grid(ui, &v, &v.daily, 200.0, 190.0);
        }
    });

    if !v.night_market.is_empty() {
        theme::card(ui, None, |ui| {
            section_title(ui, "Night Market", v.night_market_ends_at, now, "Ends in");
            let unseen = v.night_market.iter().filter(|i| i.unseen).count();
            if unseen > 0 {
                ui.label(
                    RichText::new(format!(
                        "{unseen} of these aren't revealed in the game yet. Flipping them in VALORANT doesn't change the price."
                    ))
                    .size(11.5)
                    .color(p.faint),
                );
                ui.add_space(4.0);
            }
            tile_grid(ui, &v, &v.night_market, 180.0, 190.0);
        });
    }

    for b in &v.bundles {
        theme::card(ui, None, |ui| {
            // Banner: art with the name, price and time left over it.
            let w = ui.available_width();
            let (banner, _) = ui.allocate_exact_size(vec2(w, (w * 0.28).clamp(120.0, 240.0)), Sense::hover());
            theme::image_cover(ui, banner, Some(b.art.as_str()), 1920.0 / 1080.0, 12);
            if b.art.is_empty() {
                let painter = ui.painter_at(banner);
                for (i, a) in [0.20, 0.12, 0.07].iter().enumerate() {
                    let x = banner.left() + banner.width() * (0.45 + 0.17 * i as f32);
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            pos2(x, banner.bottom()),
                            pos2(x + banner.width() * 0.22, banner.top()),
                            pos2(x + banner.width() * 0.36, banner.top()),
                            pos2(x + banner.width() * 0.14, banner.bottom()),
                        ],
                        p.accent.gamma_multiply(*a),
                        Stroke::NONE,
                    ));
                }
            }
            let shade = Rect::from_min_max(pos2(banner.left(), banner.bottom() - 64.0), banner.max);
            ui.painter().rect_filled(
                shade,
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 12,
                    se: 12,
                },
                Color32::from_black_alpha(140),
            );
            ui.painter().text(
                shade.left_center() + vec2(18.0, -9.0),
                Align2::LEFT_CENTER,
                &b.name,
                theme::semibold(20.0),
                Color32::WHITE,
            );
            if b.ends_at > 0 {
                ui.painter().text(
                    shade.left_center() + vec2(18.0, 14.0),
                    Align2::LEFT_CENTER,
                    format!("Featured bundle · leaves in {}", fmt_countdown(b.ends_at - now)),
                    egui::FontId::proportional(11.5),
                    Color32::from_white_alpha(200),
                );
            }
            if let Some(price) = &b.price {
                let right = shade.right_center() - vec2(18.0, 0.0);
                let icon = v.currency_icon(&price.currency).map(str::to_string);
                let end = paint_price(
                    ui,
                    right,
                    price.amount,
                    &price.currency,
                    icon.as_deref(),
                    16.0,
                    Color32::WHITE,
                );
                if let Some(base) = b.base_price {
                    ui.painter().text(
                        pos2(end - 8.0, right.y),
                        Align2::RIGHT_CENTER,
                        format_amount(base as i64),
                        egui::FontId::proportional(12.0),
                        Color32::from_white_alpha(150),
                    );
                    strike(
                        ui,
                        pos2(end - 8.0, right.y),
                        &format_amount(base as i64),
                        12.0,
                        Color32::from_white_alpha(150),
                    );
                }
            }
            ui.add_space(10.0);
            tile_grid(ui, &v, &b.items, 170.0, 170.0);
        });
    }

    if !v.accessories.is_empty() {
        theme::card(ui, None, |ui| {
            section_title(ui, "Accessory store", v.accessories_ends_at, now, "Refreshes in");
            tile_grid(ui, &v, &v.accessories, 170.0, 150.0);
        });
    }

    ui.label(
        RichText::new("Prices and items come straight from your own Riot store. Buy things in the game; Tracked never makes purchases.")
            .size(11.0)
            .color(p.faint),
    );
}

fn section_title(ui: &mut Ui, title: &str, ends_at: i64, now: i64, what: &str) {
    let p = pal(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::semibold(15.0)).color(p.text));
        if ends_at > now {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(fmt_countdown(ends_at - now))
                        .font(theme::semibold(12.5))
                        .color(p.text),
                );
                ui.label(RichText::new(what).size(11.5).color(p.weak));
            });
        }
    });
    ui.add_space(8.0);
}

fn balance_chip(ui: &mut Ui, icon: &str, name: &str, amount: i64, currency: &str) {
    let p = pal(ui.ctx());
    let text = format_amount(amount);
    let galley = ui.painter().layout_no_wrap(text.clone(), theme::semibold(12.5), p.text);
    let w = galley.size().x + 44.0;
    let (r, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    ui.painter().rect(
        r,
        CornerRadius::same(15),
        p.card,
        Stroke::new(1.0_f32, p.stroke),
        egui::StrokeKind::Inside,
    );
    let ic = Rect::from_center_size(r.left_center() + vec2(17.0, 0.0), vec2(16.0, 16.0));
    currency_icon(ui, ic, icon, currency);
    ui.painter().galley(
        pos2(r.left() + 31.0, r.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    resp.on_hover_text(name);
}

/// The currency's icon, or a small lettered disc when there's no artwork.
pub(super) fn currency_icon(ui: &Ui, rect: Rect, icon: &str, currency: &str) {
    if icon.is_empty() {
        let p = pal(ui.ctx());
        let short = currency_short(currency);
        ui.painter()
            .circle_filled(rect.center(), rect.width() / 2.0, p.accent.gamma_multiply(0.85));
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            short.chars().next().unwrap_or('¤').to_string(),
            theme::semibold(rect.width() * 0.6),
            Color32::WHITE,
        );
    } else {
        egui::Image::new(icon.to_string()).paint_at(ui, rect);
    }
}

/// "12,345".
pub fn format_amount(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// Paint "[icon] 1,775" ending at `right`; returns the x where it starts.
fn paint_price(
    ui: &Ui,
    right: egui::Pos2,
    amount: u32,
    currency: &str,
    icon: Option<&str>,
    size: f32,
    color: Color32,
) -> f32 {
    let text = format_amount(amount as i64);
    let galley = ui.painter().layout_no_wrap(text, theme::semibold(size), color);
    let tw = galley.size().x;
    ui.painter()
        .galley(pos2(right.x - tw, right.y - galley.size().y / 2.0), galley, color);
    let ic = Rect::from_center_size(pos2(right.x - tw - 4.0 - size * 0.5, right.y), vec2(size, size));
    currency_icon(ui, ic, icon.unwrap_or(""), currency);
    ic.left()
}

fn strike(ui: &Ui, right: egui::Pos2, text: &str, size: f32, color: Color32) {
    let w = ui
        .painter()
        .layout_no_wrap(text.to_string(), egui::FontId::proportional(size), color)
        .size()
        .x;
    ui.painter().line_segment(
        [pos2(right.x - w, right.y), pos2(right.x, right.y)],
        Stroke::new(1.0_f32, color),
    );
}

/// `RRGGBB` / `RRGGBBAA` hex (valorant-api colours).
pub(super) fn hex_color(s: &str) -> Option<Color32> {
    let b = |i: usize| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    match s.len() {
        6 | 8 => Some(Color32::from_rgb(b(0)?, b(2)?, b(4)?)),
        _ => None,
    }
}

pub(super) fn rarity_color(r: Option<&Rarity>, p: &Palette) -> Color32 {
    r.and_then(|r| hex_color(&r.color)).unwrap_or(p.weak)
}

/// Lay items out in rows of equal-width tiles.
pub(super) fn grid<T>(ui: &mut Ui, items: &[T], min_w: f32, h: f32, mut tile: impl FnMut(&mut Ui, Rect, &T)) {
    let w = ui.available_width();
    let gap = 12.0;
    let per_row = ((w + gap) / (min_w + gap)).floor().max(1.0) as usize;
    let per_row = per_row.min(items.len().max(1));
    let tw = (w - gap * (per_row as f32 - 1.0)) / per_row as f32;
    for chunk in items.chunks(per_row) {
        let (row, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        for (i, it) in chunk.iter().enumerate() {
            let r = Rect::from_min_size(row.min + vec2(i as f32 * (tw + gap), 0.0), vec2(tw, h));
            tile(ui, r, it);
        }
        ui.add_space(gap - ui.spacing().item_spacing.y);
    }
}

fn tile_grid(ui: &mut Ui, v: &StoreView, items: &[ShopItem], min_w: f32, h: f32) {
    grid(ui, items, min_w, h, |ui, r, it| item_tile(ui, r, it, v));
}

fn loading_tiles(ui: &mut Ui, n: usize, h: f32) {
    let slots = vec![(); n];
    grid(ui, &slots, 200.0, h, |ui, r, _| theme::skeleton(ui, r, 14));
}

/// One store tile: rarity-tinted panel, artwork, name, rarity and price.
fn item_tile(ui: &mut Ui, rect: Rect, it: &ShopItem, v: &StoreView) {
    let p = pal(ui.ctx());
    let resp = ui.interact(
        rect,
        ui.id().with(("tile", &it.item_id, rect.min.x as i32)),
        Sense::hover(),
    );
    let color = rarity_color(it.rarity.as_ref(), &p);
    let hover = theme::glide(
        ui.ctx(),
        resp.id.with("h"),
        if resp.hovered() { 1.0 } else { 0.0 },
        0.12,
    );
    ui.painter().rect_filled(rect, CornerRadius::same(14), p.well);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(14), color.gamma_multiply(0.10 + 0.08 * hover));
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(14),
        Stroke::new(1.0_f32, color.gamma_multiply(0.35 + 0.4 * hover)),
        egui::StrokeKind::Inside,
    );
    let painter = ui.painter_at(rect);

    // Artwork area.
    let art = Rect::from_min_max(
        rect.min + vec2(14.0, 14.0),
        pos2(rect.right() - 14.0, rect.bottom() - 62.0),
    );
    if it.icon.is_empty() {
        super::icons::paint(
            &painter,
            Rect::from_center_size(art.center(), vec2(34.0, 34.0)),
            color.gamma_multiply(0.6),
            kind_icon(it),
        );
    } else {
        ui.put(art, egui::Image::new(it.icon.clone()).fit_to_exact_size(art.size()));
    }

    // Badges.
    let mut bx = rect.right() - 10.0;
    let mut badge = |text: &str, fill: Color32| {
        let g = painter.layout_no_wrap(text.to_string(), theme::semibold(10.0), Color32::WHITE);
        let r = Rect::from_min_size(
            pos2(bx - g.size().x - 14.0, rect.top() + 10.0),
            vec2(g.size().x + 14.0, 18.0),
        );
        painter.rect_filled(r, CornerRadius::same(9), fill);
        painter.galley(r.min + vec2(7.0, 9.0 - g.size().y / 2.0), g, Color32::WHITE);
        bx = r.left() - 6.0;
    };
    if it.owned {
        badge("Owned", p.win);
    }
    if it.discount_pct > 0 {
        badge(&format!("-{}%", it.discount_pct), p.accent);
    }
    if it.unseen {
        badge("Hidden in game", Color32::from_gray(90));
    }
    if it.quantity > 1 {
        badge(&format!("×{}", it.quantity), Color32::from_gray(90));
    }

    // Name and rarity / kind.
    let name_pos = pos2(rect.left() + 14.0, rect.bottom() - 44.0);
    painter.text(name_pos, Align2::LEFT_CENTER, &it.name, theme::semibold(13.0), p.text);
    let sub = match &it.rarity {
        Some(r) if !r.name.is_empty() => r.name.clone(),
        _ => it.kind.label().to_string(),
    };
    let sub_pos = pos2(rect.left() + 14.0, rect.bottom() - 22.0);
    if let Some(r) = it.rarity.as_ref().filter(|r| !r.icon.is_empty()) {
        let ic = Rect::from_center_size(sub_pos + vec2(7.0, 0.0), vec2(14.0, 14.0));
        egui::Image::new(r.icon.clone()).paint_at(ui, ic);
        painter.text(
            sub_pos + vec2(19.0, 0.0),
            Align2::LEFT_CENTER,
            sub,
            egui::FontId::proportional(11.5),
            color,
        );
    } else {
        painter.circle_filled(sub_pos + vec2(4.0, 0.0), 3.5, color);
        painter.text(
            sub_pos + vec2(13.0, 0.0),
            Align2::LEFT_CENTER,
            sub,
            egui::FontId::proportional(11.5),
            p.weak,
        );
    }

    if let Some(price) = &it.price {
        let right = pos2(rect.right() - 14.0, sub_pos.y);
        let start = paint_price(
            ui,
            right,
            price.amount,
            &price.currency,
            v.currency_icon(&price.currency),
            13.0,
            p.text,
        );
        if let Some(base) = it.base_price {
            let at = pos2(start - 6.0, right.y);
            painter.text(
                at,
                Align2::RIGHT_CENTER,
                format_amount(base as i64),
                egui::FontId::proportional(11.0),
                p.faint,
            );
            strike(ui, at, &format_amount(base as i64), 11.0, p.faint);
        }
    }
    resp.on_hover_text(format!("{} · {}", it.name, it.kind.label()));
}

fn kind_icon(it: &ShopItem) -> super::icons::Icon {
    use super::icons::Icon;
    use crate::shop::ItemKind;
    match it.kind {
        ItemKind::Skin => Icon::Swords,
        ItemKind::Buddy => Icon::Medal,
        ItemKind::Spray => Icon::Chat,
        ItemKind::Card | ItemKind::Title => Icon::Users,
        _ => Icon::Store,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_and_colors() {
        assert_eq!(format_amount(0), "0");
        assert_eq!(format_amount(875), "875");
        assert_eq!(format_amount(12345), "12,345");
        assert_eq!(format_amount(1_234_567), "1,234,567");
        assert_eq!(hex_color("d1548dff"), Some(Color32::from_rgb(0xd1, 0x54, 0x8d)));
        assert_eq!(hex_color("nope"), None);
    }
}
