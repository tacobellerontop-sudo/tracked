//! The startup screen / connection checklist.
//!
//! Shown full-window at launch until every required step passes, then the app
//! continues to the home screen on its own. Afterwards it stays reachable as
//! the "Connection" page (and by clicking the status bar).

use std::time::Duration;

use egui::{Color32, RichText};

use super::widgets;
use crate::app::AppState;
use crate::diagnostics::{Step, StepState};
use crate::ipc::{Intent, IntentTx};

/// How long the "connected" state shows before moving on.
pub const AUTO_CONTINUE: Duration = Duration::from_millis(1200);

const PENDING: Color32 = Color32::from_rgb(110, 110, 110);
const WARN: Color32 = Color32::from_rgb(230, 180, 60);

/// Full-window startup page. Returns once the user (or the timer) moves on.
pub fn startup(ctx: &egui::Context, state: &mut AppState, intents: &IntentTx) {
    if state.connected_since.is_some_and(|t| t.elapsed() >= AUTO_CONTINUE) {
        state.finish_startup();
        return;
    }
    // Keep animating the spinner / countdown.
    ctx.request_repaint_after(Duration::from_millis(150));
    egui::CentralPanel::default().show(ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let width = 620.0_f32.min(ui.available_width() - 32.0);
            let margin = ((ui.available_width() - width) / 2.0).max(16.0);
            ui.horizontal(|ui| {
                ui.add_space(margin);
                ui.vertical(|ui| {
                    ui.set_max_width(width);
                    ui.add_space(36.0);
                    let p = super::theme::pal(ui.ctx());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                        super::icons::logo(ui.painter(), r, p.accent);
                        ui.label(RichText::new("A S C E N D").size(18.0).color(p.text));
                    });
                    ui.add_space(14.0);
                    let (headline, color) = headline(state);
                    ui.label(RichText::new(headline).size(16.0).color(color));
                    ui.add_space(18.0);
                    checklist(ui, state);
                    ui.add_space(16.0);
                    actions(ui, state, intents, true);
                });
            });
        });
    });
}

/// The same checklist inside the normal layout.
pub fn ui(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx) {
    ui.heading("Connection");
    let (headline, color) = headline(state);
    ui.label(RichText::new(headline).color(color));
    ui.add_space(10.0);
    ui.set_max_width(680.0);
    checklist(ui, state);
    ui.add_space(12.0);
    actions(ui, state, intents, false);
}

fn headline(state: &AppState) -> (String, Color32) {
    let r = &state.connection;
    if r.connected() {
        return ("Connected to VALORANT".into(), widgets::WIN);
    }
    match r.failure() {
        Some((step @ (Step::Lockfile | Step::Region), _, _)) => (step.failure_headline().into(), WARN),
        Some((step, _, _)) => (step.failure_headline().into(), widgets::LOSS),
        None => ("Connecting to the Riot Client…".into(), Color32::GRAY),
    }
}

fn checklist(ui: &mut egui::Ui, state: &AppState) {
    let p = super::theme::pal(ui.ctx());
    super::theme::card_frame(&p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 3.0;
        for (step, st) in &state.connection.steps {
            ui.horizontal_top(|ui| {
                indicator(ui, st);
                ui.vertical(|ui| {
                    let mut title = RichText::new(step.title()).strong();
                    if !step.required() {
                        title = title.color(ui.visuals().weak_text_color());
                    }
                    ui.horizontal(|ui| {
                        ui.label(title);
                        if !step.required() {
                            ui.weak("(optional)");
                        }
                    });
                    match st {
                        StepState::Pending => {
                            ui.weak("Waiting…");
                        }
                        StepState::Running => {
                            ui.weak("Checking…");
                        }
                        StepState::Ok(m) => {
                            ui.weak(m);
                        }
                        StepState::Warn(m) => {
                            ui.colored_label(WARN, m);
                        }
                        StepState::Failed { message, hint } => {
                            ui.colored_label(widgets::LOSS, message);
                            ui.label(RichText::new(hint).italics());
                        }
                    }
                });
            });
            ui.add_space(6.0);
        }
    });
}

fn indicator(ui: &mut egui::Ui, st: &StepState) {
    let size = egui::vec2(18.0, 18.0);
    if matches!(st, StepState::Running) {
        ui.add_sized(size, egui::Spinner::new().size(14.0));
        return;
    }
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let c = rect.center();
    let p = ui.painter();
    match st {
        StepState::Ok(_) => {
            p.circle_filled(c, 8.0, widgets::WIN);
            // Check mark.
            let stroke = egui::Stroke::new(2.0_f32, Color32::WHITE);
            p.line_segment([c + egui::vec2(-4.0, 0.0), c + egui::vec2(-1.0, 3.0)], stroke);
            p.line_segment([c + egui::vec2(-1.0, 3.0), c + egui::vec2(4.5, -3.5)], stroke);
        }
        StepState::Warn(_) => {
            p.circle_filled(c, 8.0, WARN);
            p.text(
                c,
                egui::Align2::CENTER_CENTER,
                "!",
                egui::FontId::proportional(12.0),
                Color32::BLACK,
            );
        }
        StepState::Failed { .. } => {
            p.circle_filled(c, 8.0, widgets::LOSS);
            let stroke = egui::Stroke::new(2.0_f32, Color32::WHITE);
            p.line_segment([c + egui::vec2(-3.5, -3.5), c + egui::vec2(3.5, 3.5)], stroke);
            p.line_segment([c + egui::vec2(3.5, -3.5), c + egui::vec2(-3.5, 3.5)], stroke);
        }
        StepState::Pending | StepState::Running => {
            p.circle_stroke(c, 7.0, egui::Stroke::new(1.5_f32, PENDING));
        }
    }
}

fn actions(ui: &mut egui::Ui, state: &mut AppState, intents: &IntentTx, startup: bool) {
    let log_dir = crate::config::cache_dir().map(|d| d.join("logs"));
    ui.horizontal_wrapped(|ui| {
        if state.connection.connected() {
            if startup && ui.button("Continue").clicked() {
                state.finish_startup();
            }
        } else if ui.button("Retry now").clicked() {
            intents.send(Intent::Bootstrap);
        }
        if ui
            .button("Copy diagnostics")
            .on_hover_text("Copies this checklist (no passwords or tokens)")
            .clicked()
        {
            ui.ctx().copy_text(state.connection.to_text(log_dir.as_deref()));
            state.pending_toasts.push("Diagnostics copied to the clipboard".into());
        }
        if let Some(dir) = &log_dir {
            if ui.button("Open logs folder").clicked() {
                open_folder(dir);
            }
        }
        if startup && !state.connection.connected() && ui.button("Continue without connecting").clicked() {
            state.finish_startup();
        }
    });
    if startup && !state.connection.connected() {
        ui.add_space(6.0);
        ui.weak("This page updates by itself and opens the app as soon as everything is connected.");
    }
}

fn open_folder(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(opener).arg(dir).spawn() {
        tracing::warn!("couldn't open {}: {e}", dir.display());
    }
}
