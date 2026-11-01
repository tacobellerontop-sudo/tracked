//! Weapons page: your numbers with each gun over the loaded matches, plus the
//! per-match breakdown shown on the match screen.

use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, Layout, Rect, RichText, Sense, Stroke, Ui};

use super::theme::{self, pal, Palette};
use crate::app::AppState;
use crate::content::weapons::{self, Class};
use crate::content::Catalog;
use crate::riot::models::WeaponStat;
use crate::stats::{self, WeaponTotals};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Sort {
    #[default]
    Kills,
    Headshots,
    Damage,
}

impl Sort {
    const ALL: [Sort; 3] = [Sort::Kills, Sort::Headshots, Sort::Damage];

    fn label(self) -> &'static str {
        match self {
            Sort::Kills => "Kills",
            Sort::Headshots => "Headshot %",
            Sort::Damage => "Damage / round",
        }
    }
}

/// Colours for the kill-share bar, in order of weapon rank.
fn series(p: &Palette, i: usize) -> Color32 {
    const LIGHT: [Color32; 6] = [
        Color32::from_rgb(240, 62, 78),
        Color32::from_rgb(20, 168, 132),
        Color32::from_rgb(76, 126, 230),
        Color32::from_rgb(222, 150, 30),
        Color32::from_rgb(150, 92, 220),
        Color32::from_rgb(222, 86, 164),
    ];
    const DARK: [Color32; 6] = [
        Color32::from_rgb(255, 70, 85),
        Color32::from_rgb(45, 212, 168),
        Color32::from_rgb(98, 150, 255),
        Color32::from_rgb(240, 182, 64),
        Color32::from_rgb(176, 124, 255),
        Color32::from_rgb(244, 114, 182),
    ];
    let set = if p.dark { DARK } else { LIGHT };
    set.get(i).copied().unwrap_or(p.faint)
}

fn class_color(p: &Palette, c: Class) -> Color32 {
    match c {
        Class::Rifle => p.accent,
        Class::Sniper => series(p, 2),
        Class::Smg => series(p, 1),
        Class::Shotgun => series(p, 3),
        Class::Heavy => series(p, 4),
        Class::Sidearm => series(p, 5),
        Class::Melee | Class::Ability | Class::Other => p.weak,
    }
}

pub fn ui(ui: &mut Ui, state: &mut AppState) {
    let p = pal(ui.ctx());
    let Some(career) = state.career.clone() else {
        theme::card(ui, None, |ui| {
            ui.weak("Start VALORANT to see your weapon stats.");
        });
        return;
    };
    let cat = state.catalog.clone();
    let refs = stats::loaded(career.matches.iter().map(|e| &e.match_id), &state.matches);
    let all = stats::weapons(&career.puuid, &refs);
    let total_kills: u32 = all.iter().map(|w| w.stat.kills).sum();

    let sort_id = egui::Id::new("weapons-sort");
    let mut sort: Sort = ui.ctx().data(|d| d.get_temp(sort_id)).unwrap_or_default();

    ui.horizontal(|ui| {
        theme::page_title(
            ui,
            "Weapons",
            &format!("last {} matches · {} kills", refs.len(), total_kills),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            for s in Sort::ALL.iter().rev() {
                if theme::pill(ui, s.label(), sort == *s).clicked() {
                    sort = *s;
                }
            }
            ui.label(RichText::new("Sort by").size(11.5).color(p.faint));
        });
    });
    ui.ctx().data_mut(|d| d.insert_temp(sort_id, sort));

    if all.iter().all(|w| w.stat.kills == 0 && w.stat.rounds == 0) {
        theme::card(ui, None, |ui| {
            ui.weak("No weapon data yet. It appears as your matches load.");
        });
        return;
    }

    // The signature weapon is the gun with the most kills (abilities and the knife don't count).
    if let Some((rank, top)) = all
        .iter()
        .enumerate()
        .find(|(_, w)| weapons::is_gun(&w.stat.weapon_id) && w.stat.kills > 0)
    {
        signature(ui, &cat, top, rank, &all);
    }

    let mut list: Vec<(usize, &WeaponTotals)> = all
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            let name = cat.weapon_name(&w.stat.weapon_id);
            let class = weapons::class(&w.stat.weapon_id).label();
            let q = state.search.trim().to_lowercase();
            q.is_empty() || name.to_lowercase().contains(&q) || class.to_lowercase().contains(&q)
        })
        .collect();
    match sort {
        Sort::Kills => {}
        Sort::Headshots => list.sort_by(|a, b| {
            hs_or_none(b.1)
                .unwrap_or(-1.0)
                .total_cmp(&hs_or_none(a.1).unwrap_or(-1.0))
        }),
        Sort::Damage => list.sort_by(|a, b| {
            b.1.damage_per_round()
                .unwrap_or(-1.0)
                .total_cmp(&a.1.damage_per_round().unwrap_or(-1.0))
        }),
    }

    let w = ui.available_width();
    let per_row = ((w + 16.0) / 280.0).floor().max(1.0) as usize;
    let cw = (w - 16.0 * (per_row as f32 - 1.0)) / per_row as f32;
    for chunk in list.chunks(per_row) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for (rank, wt) in chunk {
                ui.allocate_ui_with_layout(vec2(cw, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(cw);
                    weapon_card(ui, &cat, wt, *rank);
                });
            }
        });
    }
    ui.label(
        RichText::new(
            "Kills are exact. Riot doesn't record which gun each hit came from, so hits on a player you killed \
             count for the gun that killed them, and other hits count for the gun you bought that round.",
        )
        .size(11.0)
        .color(p.faint),
    );
}

fn hs_or_none(w: &WeaponTotals) -> Option<f32> {
    (w.stat.hits() > 0 && weapons::is_gun(&w.stat.weapon_id)).then(|| w.stat.hs_pct())
}

/// Weapon artwork from valorant-api when we have it; otherwise the name as a
/// big watermark. Either way on a tinted gradient panel.
fn art(ui: &mut Ui, cat: &Catalog, id: &str, rect: Rect, tint: Color32, big: bool) {
    let p = pal(ui.ctx());
    let radius = CornerRadius::same(12);
    ui.painter().rect_filled(rect, radius, p.well);
    ui.painter().rect_filled(rect, radius, tint.gamma_multiply(0.12));
    // Diagonal slash, a nod to the game's UI.
    let slash = vec![
        pos2(rect.right() - rect.width() * 0.32, rect.top()),
        pos2(rect.right() - rect.width() * 0.20, rect.top()),
        pos2(rect.right() - rect.width() * 0.34, rect.bottom()),
        pos2(rect.right() - rect.width() * 0.46, rect.bottom()),
    ];
    ui.painter().add(egui::Shape::convex_polygon(
        slash,
        tint.gamma_multiply(0.10),
        Stroke::NONE,
    ));
    let icon = cat
        .weapon(id)
        .map(|w| {
            if w.display_icon.is_empty() {
                &w.icon
            } else {
                &w.display_icon
            }
        })
        .filter(|u| !u.is_empty());
    match icon {
        Some(uri) => {
            let area = rect.shrink2(vec2(rect.width() * 0.08, rect.height() * 0.16));
            ui.put(
                area,
                egui::Image::new(uri.clone())
                    .max_size(area.size())
                    .maintain_aspect_ratio(true),
            );
        }
        None => {
            let name = cat.weapon_name(id).to_uppercase();
            let size = if big { 44.0 } else { 26.0 };
            ui.painter().text(
                rect.left_center() + vec2(16.0, 0.0),
                Align2::LEFT_CENTER,
                name,
                theme::semibold(size),
                p.text.gamma_multiply(if p.dark { 0.10 } else { 0.08 }),
            );
            let ic = Rect::from_center_size(
                rect.right_center() - vec2(rect.height() * 0.55, 0.0),
                vec2(rect.height() * 0.5, rect.height() * 0.5),
            );
            let glyph = match weapons::class(id) {
                Class::Ability => super::icons::Icon::Swords,
                Class::Melee => super::icons::Icon::Swords,
                _ => super::icons::Icon::Target,
            };
            super::icons::paint(ui.painter(), ic, tint.gamma_multiply(0.85), glyph);
        }
    }
}

fn signature(ui: &mut Ui, cat: &Catalog, top: &WeaponTotals, rank: usize, all: &[WeaponTotals]) {
    let p = pal(ui.ctx());
    let tint = series(&p, rank);
    theme::card(ui, None, |ui| {
        ui.horizontal_top(|ui| {
            let art_w = (ui.available_width() * 0.42).clamp(220.0, 460.0);
            let (r, _) = ui.allocate_exact_size(vec2(art_w, 150.0), Sense::hover());
            art(ui, cat, &top.stat.weapon_id, r, tint, true);
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    theme::eyebrow(ui, "Signature weapon");
                    let c = weapons::class(&top.stat.weapon_id);
                    theme::chip(ui, c.label(), class_color(&p, c));
                });
                ui.label(
                    RichText::new(cat.weapon_name(&top.stat.weapon_id))
                        .font(theme::semibold(30.0))
                        .color(p.text),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 28.0;
                    big_stat(ui, "Kills", top.stat.kills.to_string(), p.text);
                    big_stat(ui, "Kill share", format!("{:.0}%", top.kill_share), tint);
                    big_stat(
                        ui,
                        "Headshot %",
                        hs_or_none(top)
                            .map(|h| format!("{h:.1}%"))
                            .unwrap_or_else(|| "–".into()),
                        p.text,
                    );
                    big_stat(
                        ui,
                        "Dmg / round",
                        top.damage_per_round()
                            .map(|d| format!("{d:.0}"))
                            .unwrap_or_else(|| "–".into()),
                        p.text,
                    );
                });
            });
            if ui.available_width() > 240.0 {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    ui.set_max_width(260.0);
                    ui.vertical(|ui| highlights(ui, cat, all));
                });
            }
        });
        ui.add_space(14.0);
        kill_share_bar(ui, cat, all);
    });
}

/// Best gun by aim, damage and kill rate (with enough data to mean something).
fn highlights(ui: &mut Ui, cat: &Catalog, all: &[WeaponTotals]) {
    let p = pal(ui.ctx());
    let guns = || all.iter().filter(|w| weapons::is_gun(&w.stat.weapon_id));
    let best = |f: &dyn Fn(&WeaponTotals) -> Option<f32>| {
        guns()
            .filter_map(|w| f(w).map(|v| (w, v)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    };
    let rows = [
        (
            "Sharpest aim",
            best(&|w| (w.stat.hits() >= 30).then(|| w.stat.hs_pct())).map(|(w, v)| (w, format!("{v:.0}% HS"))),
        ),
        (
            "Hardest hitter",
            best(&|w| w.damage_per_round().filter(|_| w.stat.rounds >= 5))
                .map(|(w, v)| (w, format!("{v:.0} dmg/round"))),
        ),
        (
            "Most lethal",
            best(&|w| w.kills_per_round().filter(|_| w.stat.rounds >= 5))
                .map(|(w, v)| (w, format!("{v:.2} kills/round"))),
        ),
    ];
    theme::eyebrow(ui, "Highlights");
    ui.add_space(2.0);
    for (label, row) in rows {
        let Some((w, value)) = row else { continue };
        egui::Frame::NONE
            .fill(p.well.gamma_multiply(0.7))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new(label).size(10.5).color(p.weak));
                        ui.label(
                            RichText::new(cat.weapon_name(&w.stat.weapon_id))
                                .font(theme::semibold(13.5))
                                .color(p.text),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(value).font(theme::medium(12.5)).color(p.accent));
                    });
                });
            });
    }
}

fn big_stat(ui: &mut Ui, label: &str, value: String, color: Color32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        theme::eyebrow(ui, label);
        ui.label(RichText::new(value).font(theme::semibold(24.0)).color(color));
    });
}

/// One bar split by each weapon's share of kills, with a legend.
fn kill_share_bar(ui: &mut Ui, cat: &Catalog, all: &[WeaponTotals]) {
    let p = pal(ui.ctx());
    let total: u32 = all.iter().map(|w| w.stat.kills).sum();
    if total == 0 {
        return;
    }
    theme::eyebrow(ui, "Where your kills come from");
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(5), p.well);
    let t = theme::intro(ui.ctx(), egui::Id::new("kill-share"), total as u64, 0.6);
    let mut x = r.left();
    let shown = 6;
    let rest: u32 = all.iter().skip(shown).map(|w| w.stat.kills).sum();
    let segments = all
        .iter()
        .take(shown)
        .enumerate()
        .map(|(i, w)| (w.stat.kills, series(&p, i)))
        .chain(std::iter::once((rest, p.faint)));
    for (kills, color) in segments {
        if kills == 0 {
            continue;
        }
        let w = r.width() * kills as f32 / total as f32 * t;
        let seg = Rect::from_min_size(pos2(x, r.top()), vec2((w - 2.0).max(1.0), r.height()));
        ui.painter().rect_filled(seg, CornerRadius::same(5), color);
        x += w;
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        for (i, w) in all.iter().take(shown).enumerate().filter(|(_, w)| w.stat.kills > 0) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let (d, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(d.center(), 4.0, series(&p, i));
                ui.label(
                    RichText::new(cat.weapon_name(&w.stat.weapon_id))
                        .size(11.5)
                        .color(p.text),
                );
                ui.label(RichText::new(format!("{:.0}%", w.kill_share)).size(11.5).color(p.weak));
            });
        }
        if rest > 0 {
            ui.label(
                RichText::new(format!("Other {:.0}%", 100.0 * rest as f32 / total as f32))
                    .size(11.5)
                    .color(p.weak),
            );
        }
    });
}

fn weapon_card(ui: &mut Ui, cat: &Catalog, w: &WeaponTotals, rank: usize) {
    let p = pal(ui.ctx());
    let id = &w.stat.weapon_id;
    let class = weapons::class(id);
    let tint = if rank < 6 {
        series(&p, rank)
    } else {
        class_color(&p, class)
    };
    theme::card(ui, None, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 74.0), Sense::hover());
        art(ui, cat, id, r, tint, false);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(cat.weapon_name(id))
                    .font(theme::semibold(16.0))
                    .color(p.text),
            );
            theme::chip(ui, class.label(), class_color(&p, class));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("#{}", rank + 1))
                        .font(theme::medium(12.0))
                        .color(p.faint),
                );
            });
        });
        ui.add_space(4.0);
        let gun = weapons::is_gun(id);
        let cells: [(&str, String); 4] = [
            ("Kills", w.stat.kills.to_string()),
            (
                "HS %",
                hs_or_none(w).map(|h| format!("{h:.0}%")).unwrap_or_else(|| "–".into()),
            ),
            (
                "Dmg/rnd",
                w.damage_per_round()
                    .filter(|_| gun)
                    .map(|d| format!("{d:.0}"))
                    .unwrap_or_else(|| "–".into()),
            ),
            (
                "K/rnd",
                w.kills_per_round()
                    .filter(|_| gun)
                    .map(|k| format!("{k:.2}"))
                    .unwrap_or_else(|| "–".into()),
            ),
        ];
        let cell_w = (ui.available_width() - 3.0 * 8.0) / 4.0;
        ui.horizontal(|ui| {
            for (label, value) in cells {
                ui.allocate_ui_with_layout(vec2(cell_w, 40.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(cell_w);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    theme::eyebrow(ui, label);
                    ui.label(RichText::new(value).font(theme::semibold(17.0)).color(p.text));
                });
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let bw = ui.available_width() - 48.0;
            theme::bar(ui, w.kill_share / 100.0, bw, 6.0, tint);
            ui.label(RichText::new(format!("{:.0}%", w.kill_share)).size(11.0).color(p.weak));
        });
        let mut foot = Vec::new();
        if w.stat.rounds > 0 {
            foot.push(format!("{} rounds", w.stat.rounds));
        }
        foot.push(format!("{} match{}", w.matches, if w.matches == 1 { "" } else { "es" }));
        if gun && w.stat.headshot_kills > 0 {
            foot.push(format!("{} HS kills", w.stat.headshot_kills));
        }
        ui.label(RichText::new(foot.join(" · ")).size(11.0).color(p.faint));
    });
}

/// Compact per-weapon rows for one player in one match (match screen).
pub fn match_breakdown(ui: &mut Ui, cat: &Catalog, list: &[WeaponStat]) {
    let p = pal(ui.ctx());
    let shown: Vec<&WeaponStat> = list.iter().filter(|w| w.kills > 0 || w.damage > 0).collect();
    if shown.is_empty() {
        return;
    }
    let total: u32 = shown.iter().map(|w| w.kills).sum();
    let max_kills = shown.iter().map(|w| w.kills).max().unwrap_or(0).max(1);
    egui::Frame::NONE
        .fill(p.well.gamma_multiply(0.6))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                super::icons::icon(ui, super::icons::Icon::Target, 15.0, p.accent);
                ui.label(RichText::new("Your weapons").font(theme::semibold(13.5)).color(p.text));
                ui.label(RichText::new(format!("{total} kills")).size(11.5).color(p.weak));
            });
            ui.add_space(4.0);
            egui::Grid::new("match-weapons")
                .num_columns(6)
                .spacing([18.0, 6.0])
                .show(ui, |ui| {
                    for h in ["Weapon", "Kills", "", "HS %", "Damage", "Rounds"] {
                        theme::eyebrow(ui, h);
                    }
                    ui.end_row();
                    for (i, w) in shown.iter().enumerate() {
                        let gun = weapons::is_gun(&w.weapon_id);
                        ui.label(
                            RichText::new(cat.weapon_name(&w.weapon_id))
                                .font(theme::medium(12.5))
                                .color(p.text),
                        );
                        ui.label(RichText::new(w.kills.to_string()).font(theme::semibold(13.0)));
                        theme::bar(ui, w.kills as f32 / max_kills as f32, 90.0, 6.0, series(&p, i));
                        ui.label(if gun && w.hits() > 0 {
                            format!("{:.0}%", w.hs_pct())
                        } else {
                            "–".into()
                        });
                        ui.label(if w.damage > 0 {
                            w.damage.to_string()
                        } else {
                            "–".into()
                        });
                        ui.label(if w.rounds > 0 {
                            w.rounds.to_string()
                        } else {
                            "–".into()
                        });
                        ui.end_row();
                    }
                });
        });
}
