//! `eframe::App` implementation and the UI-thread state.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::{Settings, Theme};
use crate::content::Catalog;
use std::collections::HashSet;
use std::time::Instant;

use crate::backend::models::{
    AppConfig, BackendUser, Conversation, DmMessage, LobbyInfo, MinVersionState, PresenceEntry,
};
use crate::crosshair::CrosshairCode;
use crate::ipc::{
    BackendStatus, CareerSummary, ClientStatus, Intent, IntentTx, PlayerCard, Update, UpdateRx, UpdateStatus,
};
use crate::riot::models::{LiveMatch, MatchDetail, MatchId, Puuid};
use crate::riot::pd::PlayerProbe;
use crate::riot::presence::GamePresence;
use crate::riot::RiotId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Live,
    Matches,
    /// Pick a match to watch as a 2D replay.
    Replays,
    Agents,
    Lookup,
    Crosshairs,
    Social,
    Settings,
    MatchDetail,
    /// 2D top-down replay of `AppState::replay`.
    Replay,
    /// The connection checklist (also the startup screen).
    Connection,
}

/// State of the 2D replay screen.
#[derive(Default)]
pub struct ReplayView {
    pub match_id: Option<MatchId>,
    /// `None` while loading.
    pub data: Option<Result<Arc<crate::replay::MatchReplay>, String>>,
    pub round: usize,
    /// Playback clock within the round (ms since round start); `None` means
    /// "start of the round".
    pub t: Option<f32>,
    pub playing: bool,
    /// Playback speed (1.0 = real time).
    pub speed: f32,
    /// Whose team counts as "yours" for colours.
    pub perspective: Option<Puuid>,
    pub return_to: Option<Screen>,
}

/// Which Social tab is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SocialTab {
    /// Riot friends + whispers (built in).
    #[default]
    Friends,
    /// The optional Tracked backend.
    Tracked,
}

/// Built-in social: Riot friends, their live status, whispers.
#[derive(Default)]
pub struct RiotSocial {
    pub requested: bool,
    pub loaded: bool,
    pub friends: Vec<crate::riot::chat::Friend>,
    pub requests: Vec<crate::riot::chat::FriendRequest>,
    pub statuses: HashMap<String, crate::riot::chat::FriendStatus>,
    /// Unread whisper counts by cid.
    pub unread: HashMap<String, u32>,
    pub messages: HashMap<String, Vec<crate::riot::chat::ChatMessage>>,
    /// Open conversation (cid = the friend's pid).
    pub open: Option<String>,
    pub drafts: HashMap<String, String>,
    pub sending: HashMap<String, bool>,
    pub filter: String,
    pub show_offline: bool,
    pub show_requests: bool,
    pub tab: SocialTab,
}

impl RiotSocial {
    pub fn unread_total(&self) -> u32 {
        self.unread.values().sum()
    }

    pub fn friend_by_cid(&self, cid: &str) -> Option<&crate::riot::chat::Friend> {
        self.friends.iter().find(|f| f.pid == cid)
    }

    /// Merge new messages; count unread for whispers that aren't open.
    pub fn on_messages(&mut self, msgs: Vec<crate::riot::chat::ChatMessage>, me: &str) {
        for m in msgs {
            if m.kind != "chat" {
                continue; // party / team chats aren't shown
            }
            let list = self.messages.entry(m.cid.clone()).or_default();
            if list.iter().any(|x| x.id == m.id && !m.id.is_empty()) {
                continue;
            }
            if m.puuid != me && self.open.as_deref() != Some(m.cid.as_str()) {
                *self.unread.entry(m.cid.clone()).or_default() += 1;
            }
            if m.puuid == me {
                // Our own message came back: clear the "sending" state.
                self.sending.remove(&m.cid);
            }
            list.push(m);
            list.sort_by_key(|x| x.time_ms());
        }
    }
}

/// Everything the Social screen shows (Phase 3, optional backend).
pub struct SocialState {
    pub status: BackendStatus,
    pub min_version: MinVersionState,
    pub config: Option<AppConfig>,
    pub connected: bool,
    pub friends: Vec<BackendUser>,
    pub presence: HashMap<String, PresenceEntry>,
    pub lobbies: HashMap<String, LobbyInfo>,
    pub conversations: Vec<Conversation>,
    pub messages: HashMap<String, Vec<DmMessage>>,
    pub has_more: HashMap<String, bool>,
    pub open: Option<String>,
    /// Open the next conversation that arrives (after "Message" / "Create").
    pub open_next: bool,
    pub draft: String,
    pub typing: HashMap<String, (String, Instant)>,
    pub last_typing_sent: Option<Instant>,
    pub sign_in_name: String,
    pub group_pick: HashSet<String>,
    pub group_name: String,
    pub rename: String,
    pub feedback: String,
    pub url_input: String,
}

impl SocialState {
    fn new(url: Option<String>) -> Self {
        Self {
            status: BackendStatus::Disabled,
            min_version: MinVersionState::Ok,
            config: None,
            connected: false,
            friends: Vec::new(),
            presence: HashMap::new(),
            lobbies: HashMap::new(),
            conversations: Vec::new(),
            messages: HashMap::new(),
            has_more: HashMap::new(),
            open: None,
            open_next: false,
            draft: String::new(),
            typing: HashMap::new(),
            last_typing_sent: None,
            sign_in_name: String::new(),
            group_pick: HashSet::new(),
            group_name: String::new(),
            rename: String::new(),
            feedback: String::new(),
            url_input: url.unwrap_or_default(),
        }
    }

    pub fn me(&self) -> Option<&BackendUser> {
        match &self.status {
            BackendStatus::SignedIn(u) => Some(u),
            _ => None,
        }
    }

    pub fn user_name(&self, id: &str) -> String {
        if self.me().is_some_and(|m| m.id == id) {
            return "You".into();
        }
        self.friends
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.display_name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn upsert_conversation(&mut self, c: Conversation) {
        let id = c.id.clone();
        match self.conversations.iter_mut().find(|x| x.id == c.id) {
            Some(x) => *x = c,
            None => self.conversations.insert(0, c),
        }
        if self.open_next {
            self.open_next = false;
            self.open = Some(id);
        }
    }

    fn add_messages(&mut self, conv: &str, msgs: Vec<DmMessage>) {
        let list = self.messages.entry(conv.to_string()).or_default();
        for m in msgs {
            if !list.iter().any(|x| x.id == m.id) {
                list.push(m);
            }
        }
        list.sort_by_key(|m| m.id);
    }

    fn on_message(&mut self, m: DmMessage) {
        let mine = self.me().is_some_and(|me| me.id == m.sender_id);
        let is_open = self.open.as_deref() == Some(m.conversation_id.as_str());
        self.typing.remove(&m.conversation_id);
        if let Some(c) = self.conversations.iter_mut().find(|c| c.id == m.conversation_id) {
            let newer = c.last_message.as_ref().is_none_or(|l| l.id < m.id);
            if newer {
                if !mine && !is_open {
                    c.unread += 1;
                }
                c.last_message = Some(m.clone());
            }
        }
        // Most recent conversation first.
        self.conversations
            .sort_by_key(|c| std::cmp::Reverse(c.last_message.as_ref().map(|m| m.id).unwrap_or(0)));
        self.add_messages(&m.conversation_id.clone(), vec![m]);
    }
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
    pub replay: ReplayView,
    /// Smoothed UI time per frame in ms (diagnostics).
    pub frame_ms: f32,
    /// Result of Settings → "Check replay files" (`None` while running).
    pub replay_file_report: Option<String>,
    pub live: Option<LiveMatch>,
    pub names: HashMap<Puuid, RiotId>,
    pub probes: HashMap<Puuid, Result<PlayerProbe, String>>,
    pub lookup: LookupState,
    pub crosshair: CrosshairEditor,
    pub update_status: Option<UpdateStatus>,
    /// Home performance chart range.
    pub home_range: crate::stats::Range,
    /// Top-bar search text (filters Matches / Agents; Name#TAG looks up).
    pub search: String,
    pub social: SocialState,
    pub riot_social: RiotSocial,
    pub toasts: Vec<Toast>,
    pub pending_toasts: Vec<String>,
    pub settings: Settings,
    pub last_pong: Option<u64>,
    /// "GPU · <adapter> (<backend>)", shown in Settings.
    pub renderer: String,
    /// When the current screen was opened (drives the page fade-in).
    pub screen_since: f64,
    pub last_screen: Option<Screen>,
    /// Step-by-step connection status from the worker.
    pub connection: crate::diagnostics::ConnectionReport,
    /// False while the startup screen is showing.
    pub startup_done: bool,
    /// When every required step first passed (drives the auto-continue).
    pub connected_since: Option<Instant>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            screen: Screen::Home,
            status: ClientStatus::NotRunning,
            connection: Default::default(),
            renderer: String::new(),
            screen_since: 0.0,
            last_screen: None,
            startup_done: false,
            connected_since: None,
            catalog: Arc::new(Catalog::default()),
            game: None,
            career: None,
            career_loading: false,
            matches: HashMap::new(),
            selected_match: None,
            match_perspective: None,
            return_to: Screen::Home,
            replay: ReplayView::default(),
            frame_ms: 0.0,
            replay_file_report: None,
            live: None,
            names: HashMap::new(),
            probes: HashMap::new(),
            lookup: LookupState::default(),
            crosshair: CrosshairEditor::default(),
            update_status: None,
            home_range: crate::stats::Range::Days30,
            search: String::new(),
            social: SocialState::new(settings.backend_url.clone()),
            riot_social: RiotSocial::default(),
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
            Update::Connection(r) => {
                if !r.connected() {
                    self.connected_since = None;
                } else if self.connected_since.is_none() {
                    self.connected_since = Some(Instant::now());
                }
                self.connection = r;
            }
            Update::Content(c) => self.catalog = c,
            Update::Game(g) => {
                // Jump to the live screen the moment agent select starts.
                let was_live = self.game.as_ref().is_some_and(|g| g.loop_state.is_live());
                if self.startup_done && !was_live && g.as_ref().is_some_and(|g| g.loop_state.is_live()) {
                    self.screen = Screen::Live;
                }
                self.game = g;
            }
            Update::CareerLoading(b) => self.career_loading = b,
            Update::Career(c) => self.career = Some(c),
            Update::MatchDetails(id, m) => {
                self.matches.insert(id, m);
            }
            Update::ReplayFileReport(r) => self.replay_file_report = Some(r),
            Update::Replay { match_id, result } => {
                if self.replay.match_id.as_deref() == Some(match_id.as_str()) {
                    self.replay.data = Some(result.map_err(|e| e.0));
                }
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
            Update::Notice(n) => self.pending_toasts.push(n),
            Update::RiotFriends { friends, requests } => {
                let rs = &mut self.riot_social;
                rs.friends = friends;
                rs.friends.sort_by_key(|f| f.game_name.to_lowercase());
                rs.requests = requests;
                rs.loaded = true;
            }
            Update::RiotConversations(convs) => {
                for c in convs.into_iter().filter(|c| c.kind == "chat") {
                    if c.unread_count > 0 && self.riot_social.open.as_deref() != Some(c.cid.as_str()) {
                        self.riot_social.unread.insert(c.cid, c.unread_count);
                    }
                }
            }
            Update::RiotStatuses(list) => self.riot_social.statuses.extend(list),
            Update::RiotChatHistory { cid, messages } => {
                let me = self.self_puuid().unwrap_or_default().to_string();
                self.riot_social.messages.remove(&cid);
                self.riot_social.on_messages(messages, &me);
                self.riot_social.unread.remove(&cid);
            }
            Update::RiotChat(msgs) => {
                let me = self.self_puuid().unwrap_or_default().to_string();
                self.riot_social.on_messages(msgs, &me);
            }
            Update::RiotChatFailed { cid, reason } => {
                self.riot_social.sending.remove(&cid);
                self.pending_toasts.push(format!("Message not sent: {reason}"));
            }
            Update::Backend(st) => {
                if !matches!(st, BackendStatus::SignedIn(_)) {
                    // Drop per-user data, but keep what describes the backend itself.
                    let url = self.social.url_input.clone();
                    let min = self.social.min_version.clone();
                    let config = self.social.config.take();
                    self.social = SocialState::new(Some(url));
                    self.social.min_version = min;
                    self.social.config = config;
                }
                self.social.status = st;
            }
            Update::MinVersion(m) => self.social.min_version = m,
            Update::AppConfig(c) => self.social.config = Some(c),
            Update::SocialLive(b) => self.social.connected = b,
            Update::SocialFriends(f) => self.social.friends = f,
            Update::SocialPresence(entries) => {
                for e in entries {
                    if let Some(l) = &e.lobby {
                        self.social.lobbies.insert(e.user_id.clone(), l.clone());
                    }
                    self.social.presence.insert(e.user_id.clone(), e);
                }
            }
            Update::SocialLobby { user_id, lobby } => match lobby {
                Some(l) => {
                    self.social.lobbies.insert(user_id, l);
                }
                None => {
                    self.social.lobbies.remove(&user_id);
                }
            },
            Update::DmConversations(list) => {
                self.social.conversations = list;
                if self.social.open.is_none() {
                    self.social.open = self.social.conversations.first().map(|c| c.id.clone());
                }
            }
            Update::DmConversation(c) => self.social.upsert_conversation(c),
            Update::DmPage {
                conversation_id,
                messages,
                has_more,
            } => {
                self.social.has_more.insert(conversation_id.clone(), has_more);
                self.social.add_messages(&conversation_id, messages);
            }
            Update::DmMessage(m) => self.social.on_message(m),
            Update::DmTyping {
                conversation_id,
                user_id,
            } => {
                self.social.typing.insert(conversation_id, (user_id, Instant::now()));
            }
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

    /// Open the 2D replay of a match.
    pub fn open_replay(&mut self, id: MatchId, perspective: Option<Puuid>, intents: &IntentTx) {
        if self.replay.match_id.as_deref() != Some(id.as_str()) || matches!(self.replay.data, Some(Err(_))) {
            intents.send(Intent::OpenReplay(id.clone()));
            self.replay = ReplayView {
                match_id: Some(id),
                speed: 1.0,
                ..Default::default()
            };
        }
        self.replay.perspective = perspective.or_else(|| self.self_puuid().map(str::to_string));
        self.replay.playing = false;
        if self.screen != Screen::Replay {
            self.replay.return_to = Some(self.screen);
        }
        self.screen = Screen::Replay;
    }

    /// Leave the startup screen for the home screen (Live if a game is on).
    pub fn finish_startup(&mut self) {
        self.startup_done = true;
        let live = self.game.as_ref().is_some_and(|g| g.loop_state.is_live());
        self.screen = if live { Screen::Live } else { Screen::Home };
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
    /// When the window size/maximized state last changed and isn't saved yet.
    window_dirty_since: Option<f64>,
}

impl App {
    pub fn new(
        settings: Settings,
        intents: IntentTx,
        updates: UpdateRx,
        worker: std::thread::JoinHandle<()>,
        renderer: String,
    ) -> Self {
        Self {
            state: AppState {
                renderer,
                ..AppState::new(settings)
            },
            intents,
            updates,
            worker: Some(worker),
            applied_theme: None,
            window_dirty_since: None,
        }
    }
}

impl App {
    /// Remember the window's normal size and maximized state, and save them
    /// a second after they stop changing (so a crash doesn't lose them).
    fn track_window(&mut self, ctx: &egui::Context) {
        let (rect, maximized, minimized, now) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.maximized, v.minimized, i.time)
        });
        let s = &mut self.state.settings;
        let mut changed = false;
        if minimized != Some(true) {
            if let Some(m) = maximized {
                changed |= s.window_maximized != m;
                s.window_maximized = m;
            }
            // While maximized the rect is the screen, not the size to restore to.
            if let (Some(r), false) = (rect, s.window_maximized) {
                let size = [r.width().round(), r.height().round()];
                if size[0] >= 400.0 && size[1] >= 300.0 && size != s.window_size {
                    s.window_size = size;
                    changed = true;
                }
            }
        }
        if changed {
            self.window_dirty_since = Some(now);
        }
        if let Some(since) = self.window_dirty_since {
            if now - since > 1.0 {
                self.window_dirty_since = None;
                self.state.save_settings(&self.intents);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(1100));
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let started = Instant::now();
        while let Ok(update) = self.updates.try_recv() {
            self.state.apply(update);
        }
        if self.applied_theme != Some(self.state.settings.theme) {
            crate::ui::theme::apply(ctx, self.state.settings.theme);
            self.applied_theme = Some(self.state.settings.theme);
        }
        crate::ui::theme::set_animations(ctx, self.state.settings.animations);
        self.track_window(ctx);
        crate::ui::draw(ctx, &mut self.state, &self.intents);
        // UI work per frame (not GPU time), shown in Settings; slow frames
        // are logged so lag can be diagnosed from the log file.
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        self.state.frame_ms = if self.state.frame_ms == 0.0 {
            ms
        } else {
            self.state.frame_ms * 0.9 + ms * 0.1
        };
        if ms > 50.0 {
            tracing::info!(screen = ?self.state.screen, ms = format!("{ms:.1}"), "slow frame");
        }
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
