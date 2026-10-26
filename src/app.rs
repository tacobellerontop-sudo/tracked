//! `eframe::App` implementation and the UI-thread state.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::{Settings, Theme};
use crate::content::Catalog;
use crate::crosshair::CrosshairCode;
use crate::ipc::{CareerSummary, ClientStatus, Intent, IntentTx, PlayerCard, Update, UpdateRx, UpdateStatus};
use crate::riot::models::{LiveMatch, MatchDetail, MatchId, Puuid};
use crate::riot::pd::PlayerProbe;
use crate::riot::presence::GamePresence;
use crate::riot::RiotId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Live,
    Career,
    Lookup,
    Crosshairs,
    Settings,
    MatchDetail,
}

#[derive(Default)]
pub struct LookupState {
    pub input: String,
    pub loading: bool,
    pub result: Option<Result<PlayerCard, String>>,
}

#[derive(Default)]
pub struct CrosshairEditor {
    pub input: String,
    pub name: String,
    pub parsed: Option<Result<CrosshairCode, String>>,
}

impl CrosshairEditor {
    pub fn load(&mut self, code: &str) {
        self.input = code.to_string();
        self.parsed = Some(CrosshairCode::parse(code).map_err(|e| e.to_string()));
    }
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
    /// Whose point of view the match detail screen highlights.
    pub match_perspective: Option<Puuid>,
    pub return_to: Screen,
    pub live: Option<LiveMatch>,
    pub names: HashMap<Puuid, RiotId>,
    pub probes: HashMap<Puuid, Result<PlayerProbe, String>>,
    pub lookup: LookupState,
    pub crosshair: CrosshairEditor,
    pub update_status: Option<UpdateStatus>,
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
            match_perspective: None,
            return_to: Screen::Career,
            live: None,
            names: HashMap::new(),
            probes: HashMap::new(),
            lookup: LookupState::default(),
            crosshair: CrosshairEditor::default(),
            update_status: None,
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
            Update::LiveMatch(m) => self.live = m,
            Update::Names(names) => self.names.extend(names),
            Update::Probe(p, r) => {
                self.probes.insert(p, r.map_err(|e| e.0));
            }
            Update::PlayerLookup(r) => {
                self.lookup.loading = false;
                if let Ok(card) = &r {
                    if let Some(id) = &card.riot_id {
                        self.names.insert(card.puuid.clone(), id.clone());
                    }
                }
                self.lookup.result = Some(r.map_err(|e| e.0));
            }
            Update::UpdateStatus(s) => self.update_status = Some(s),
        }
    }

    /// Open a match, highlighting `perspective` (defaults to ourselves).
    pub fn open_match(&mut self, id: MatchId, perspective: Option<Puuid>, intents: &IntentTx) {
        if !self.matches.contains_key(&id) {
            intents.send(Intent::LoadMatchDetails(id.clone()));
        }
        self.match_perspective = perspective.or_else(|| self.self_puuid().map(str::to_string));
        self.return_to = self.screen;
        self.selected_match = Some(id);
        self.screen = Screen::MatchDetail;
    }

    /// Start a lookup by PUUID (e.g. clicking a lobby player).
    pub fn lookup_puuid(&mut self, puuid: &str, intents: &IntentTx) {
        self.lookup.input = self.names.get(puuid).map(|id| id.to_string()).unwrap_or_default();
        self.lookup.loading = true;
        self.lookup.result = None;
        self.screen = Screen::Lookup;
        intents.send(Intent::LookupPuuid(puuid.to_string()));
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
