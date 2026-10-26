//! The contract between the UI thread and the worker: [`Intent`]s go down,
//! [`Update`]s come back. This replaces the IPC command bridge a web-view
//! app would use.

use std::sync::{mpsc, Arc};

use crate::backend::models::{
    AppConfig, BackendUser, Conversation, DmMessage, LobbyInfo, MinVersionState, PresenceEntry,
};
use crate::config::Settings;
use crate::content::Catalog;
use crate::error::UserFacingError;
use crate::riot::models::{LiveMatch, MatchDetail, MatchHistoryEntry, MatchId, Puuid};
use crate::riot::pd::{PlayerProbe, RankSnapshot};
use crate::riot::presence::GamePresence;
use crate::riot::RiotId;

/// Sent UI -> worker. One variant per user action / background need.
#[derive(Debug, Clone)]
pub enum Intent {
    /// Plumbing check: the worker answers with [`Update::Pong`].
    Ping(u64),
    /// Find the client and authenticate now (skips any pending backoff).
    Bootstrap,
    /// Poll the live game now instead of waiting for the next tick.
    RefreshLiveMatch,
    RefreshCareer,
    LoadMatchDetails(MatchId),
    LookupPlayer {
        name: String,
        tag: String,
    },
    LookupPuuid(Puuid),
    /// Re-probe ranks for everyone in the current party / lobby / match.
    ProbePartyMmr,
    SaveSettings(Settings),
    CheckForUpdates,

    // ---- optional backend (Phase 3)
    /// (Re)connect to the configured backend URL (after it changes).
    BackendConfigure,
    BackendSignIn {
        display_name: String,
    },
    BackendSignOut,
    SocialRefresh,
    DmOpen(String),
    DmLoadOlder {
        conversation_id: String,
        before: u64,
    },
    DmSend {
        conversation_id: String,
        text: String,
    },
    DmCreate {
        members: Vec<String>,
        name: Option<String>,
    },
    DmRename {
        conversation_id: String,
        name: String,
    },
    DmMarkRead {
        conversation_id: String,
        message_id: u64,
    },
    DmTyping(String),
    SendFeedback(String),
    Shutdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientStatus {
    /// No lockfile: the Riot client isn't running.
    NotRunning,
    /// The client is running but nobody is signed in yet.
    SignedOut,
    /// The client went away while connected (transient; next is NotRunning).
    Lost,
    /// Lockfile found but not authenticated yet (client still starting, etc.).
    Connecting(String),
    Authed {
        riot_id: Option<RiotId>,
        region: String,
    },
}

/// One point on the RR-over-time sparkline.
#[derive(Debug, Clone, PartialEq)]
pub struct RrPoint {
    pub at_unix: i64,
    pub tier: u32,
    pub rr: i32,
    pub earned: i32,
}

impl RrPoint {
    /// A monotone "ladder" value so tier changes show up on one axis.
    pub fn ladder(&self) -> i32 {
        self.tier as i32 * 100 + self.rr
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CareerSummary {
    pub puuid: String,
    pub riot_id: Option<RiotId>,
    pub rank: RankSnapshot,
    /// Oldest first.
    pub rr_history: Vec<RrPoint>,
    pub matches: Vec<MatchHistoryEntry>,
}

/// Result of a name#tag lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerCard {
    pub puuid: Puuid,
    pub riot_id: Option<RiotId>,
    pub probe: PlayerProbe,
    pub matches: Vec<MatchHistoryEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    Checking,
    UpToDate,
    NoReleases,
    Available { version: String, url: String },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendStatus {
    /// No backend URL configured: tracker-only mode.
    Disabled,
    SignedOut,
    SigningIn,
    SignedIn(BackendUser),
    Error(String),
}

/// Sent worker -> UI. One variant per thing the UI can display.
#[derive(Debug, Clone)]
pub enum Update {
    Pong(u64),
    ClientStatus(ClientStatus),
    /// Step-by-step connection checklist for the startup screen.
    Connection(crate::diagnostics::ConnectionReport),
    Content(Arc<Catalog>),
    /// Our own presence (menus / agent select / in game), `None` if unknown.
    Game(Option<GamePresence>),
    LiveMatch(Option<LiveMatch>),
    /// Resolved Riot IDs (never sent for streamer-mode players).
    Names(Vec<(Puuid, RiotId)>),
    Probe(Puuid, Result<PlayerProbe, UserFacingError>),
    PlayerLookup(Result<PlayerCard, UserFacingError>),
    UpdateStatus(UpdateStatus),
    CareerLoading(bool),
    Backend(BackendStatus),
    MinVersion(MinVersionState),
    AppConfig(AppConfig),
    /// `true` while the social websocket is connected.
    SocialLive(bool),
    SocialFriends(Vec<BackendUser>),
    SocialPresence(Vec<PresenceEntry>),
    SocialLobby {
        user_id: String,
        lobby: Option<LobbyInfo>,
    },
    DmConversations(Vec<Conversation>),
    DmConversation(Conversation),
    DmPage {
        conversation_id: String,
        messages: Vec<DmMessage>,
        has_more: bool,
    },
    DmMessage(DmMessage),
    DmTyping {
        conversation_id: String,
        user_id: String,
    },
    /// A short success message for the toast area.
    Notice(String),
    Career(CareerSummary),
    MatchDetails(MatchId, Arc<MatchDetail>),
    Error(UserFacingError),
}

/// UI-side handle for sending intents. Never blocks.
#[derive(Clone)]
pub struct IntentTx(tokio::sync::mpsc::UnboundedSender<Intent>);

impl IntentTx {
    pub fn send(&self, intent: Intent) {
        // The worker only goes away at shutdown; dropping the intent then is fine.
        let _ = self.0.send(intent);
    }
}

pub type IntentRx = tokio::sync::mpsc::UnboundedReceiver<Intent>;

/// Worker-side handle for sending updates. Wakes the UI after every push.
#[derive(Clone)]
pub struct UpdateSink {
    tx: mpsc::Sender<Update>,
    ctx: egui::Context,
}

impl UpdateSink {
    pub fn send(&self, update: Update) {
        if self.tx.send(update).is_ok() {
            self.ctx.request_repaint();
        }
    }
}

pub type UpdateRx = mpsc::Receiver<Update>;

/// Build both channels. `ctx` is used to wake the UI when updates arrive.
pub fn channels(ctx: egui::Context) -> (IntentTx, IntentRx, UpdateSink, UpdateRx) {
    let (itx, irx) = tokio::sync::mpsc::unbounded_channel();
    let (utx, urx) = mpsc::channel();
    (IntentTx(itx), irx, UpdateSink { tx: utx, ctx }, urx)
}
