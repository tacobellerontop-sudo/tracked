//! `eframe::App` implementation and the UI-thread state.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::{Settings, Theme};
use crate::content::Catalog;
use crate::ipc::{CareerSummary, ClientStatus, Intent, IntentTx, Update, UpdateRx};
use crate::riot::models::{MatchDetail, MatchId};
use crate::riot::presence::GamePresence;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Live,
    Career,
    MatchDetail,
    Settings,
}

/// A transient error message shown at the bottom of the window.
pub struct Toast {
    pub text: String,
    pub until: f64,
}

/// Plain data owned by the UI thread. Mutated only from [`Update`]s and
/// direct user input — never shared with the worker.
pub struct AppState {
    pub screen: Screen,
    pub status: ClientStatus,
    pub catalog: Arc<Catalog>,
    pub game: Option<GamePresence>,
    pub career: Option<CareerSummary>,
    pub career_loading: bool,
    pub matches: HashMap<MatchId, Arc<MatchDetail>>,
    pub selected_match: Option<MatchId>,
    pub toasts: Vec<Toast>,
    pub pending_toasts: Vec<String>,
    pub settings: Settings,
    pub last_pong: Option<u64>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            screen: Screen::Career,
            status: ClientStatus::NotRunning,
            catalog: Arc::new(Catalog::default()),
            game: None,
            career: None,
            career_loading: false,
            matches: HashMap::new(),
            selected_match: None,
            toasts: Vec::new(),
            pending_toasts: Vec::new(),
            settings,
            last_pong: None,
        }
    }

    pub fn self_puuid(&self) -> Option<&str> {
        self.career.as_ref().map(|c| c.puuid.as_str())
    }

    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Pong(n) => self.last_pong = Some(n),
            Update::ClientStatus(s) => {
                if !matches!(s, ClientStatus::Authed { .. }) {
                    self.game = None;
                }
                self.status = s;
            }
            Update::Content(c) => self.catalog = c,
            Update::Game(g) => {
                // Jump to the live screen the moment agent select starts.
                let was_live = self.game.as_ref().is_some_and(|g| g.loop_state.is_live());
                if !was_live && g.as_ref().is_some_and(|g| g.loop_state.is_live()) {
                    self.screen = Screen::Live;
                }
                self.game = g;
            }
            Update::CareerLoading(b) => self.career_loading = b,
            Update::Career(c) => self.career = Some(c),
            Update::MatchDetails(id, m) => {
                self.matches.insert(id, m);
            }
            Update::Error(e) => self.pending_toasts.push(e.0),
        }
    }

    pub fn open_match(&mut self, id: MatchId, intents: &IntentTx) {
        if !self.matches.contains_key(&id) {
            intents.send(Intent::LoadMatchDetails(id.clone()));
        }
        self.selected_match = Some(id);
        self.screen = Screen::MatchDetail;
    }

    /// Change a setting and persist it (the worker does the disk write).
    pub fn save_settings(&self, intents: &IntentTx) {
        intents.send(Intent::SaveSettings(self.settings.clone()));
    }
}

pub struct App {
    pub state: AppState,
    intents: IntentTx,
    updates: UpdateRx,
    worker: Option<std::thread::JoinHandle<()>>,
    applied_theme: Option<Theme>,
}

impl App {
    pub fn new(settings: Settings, intents: IntentTx, updates: UpdateRx, worker: std::thread::JoinHandle<()>) -> Self {
        Self {
            state: AppState::new(settings),
            intents,
            updates,
            worker: Some(worker),
            applied_theme: None,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(update) = self.updates.try_recv() {
            self.state.apply(update);
        }
        if self.applied_theme != Some(self.state.settings.theme) {
            ctx.set_visuals(match self.state.settings.theme {
                Theme::Dark => egui::Visuals::dark(),
                Theme::Light => egui::Visuals::light(),
            });
            self.applied_theme = Some(self.state.settings.theme);
        }
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            self.state.settings.window_size = [rect.width(), rect.height()];
        }
        crate::ui::draw(ctx, &mut self.state, &self.intents);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Persist the window size directly: the worker is shutting down.
        let _ = crate::config::save(&self.state.settings);
        self.intents.send(Intent::Shutdown);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}
