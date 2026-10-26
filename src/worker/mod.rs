//! The worker owns the tokio runtime and runs every piece of I/O.
//!
//! [`run`] is a single supervisor loop that:
//! * dispatches [`Intent`]s (spawning a task per piece of work),
//! * drives the connection state machine (lockfile -> authed session),
//! * keeps the presence websocket alive, falling back to polling.

pub mod demo;
pub mod social;
pub mod tasks;

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle as TaskHandle;
use tokio::time::Instant;

use crate::config::Settings;
use crate::diagnostics::{ConnectionReport, Step, StepState};
use crate::error::{ApiError, UserFacingError};
use crate::ipc::{ClientStatus, Intent, IntentRx, Update, UpdateSink};
use crate::riot::local::LocalClient;
use crate::riot::presence::{self, GamePresence, LoopState, PresenceEvent};
use crate::riot::{self, lockfile, pd, RiotId, Session};
use tasks::Ctx;

/// Lockfile check cadence while the client isn't running.
const LOCKFILE_POLL: Duration = Duration::from_millis(2500);
/// How often we confirm the client is still there once authed.
const ALIVE_CHECK: Duration = Duration::from_secs(3);
/// Presence polling cadence when the websocket is down.
const PRESENCE_POLL: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Party polling while sitting in menus (presence events also trigger it).
const MENU_LIVE_POLL: Duration = Duration::from_secs(30);

/// Spawn the worker on its own OS thread with a multi-thread tokio runtime.
pub fn spawn(intents: IntentRx, sink: UpdateSink, settings: Settings) -> JoinHandle<()> {
    spawn_mode(intents, sink, settings, false)
}

/// Like [`spawn`]; with `demo` the worker serves synthetic data instead.
pub fn spawn_mode(intents: IntentRx, sink: UpdateSink, settings: Settings, demo: bool) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("ascend-worker".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("ascend-rt")
                .build()
                .expect("failed to build tokio runtime");
            if demo {
                let ctx = Arc::new(Ctx::new(sink, settings));
                rt.block_on(demo::run(intents, ctx));
            } else {
                rt.block_on(run(intents, sink, settings));
            }
            rt.shutdown_timeout(Duration::from_secs(1));
        })
        .expect("failed to spawn worker thread")
}

enum Phase {
    Disconnected,
    Authed { pid: u32, port: u16 },
}

struct Supervisor {
    ctx: Arc<Ctx>,
    phase: Phase,
    status: Option<ClientStatus>,
    next_check: Instant,
    backoff: Duration,
    presence_tx: mpsc::UnboundedSender<PresenceEvent>,
    socket: Option<TaskHandle<()>>,
    socket_connected: bool,
    socket_backoff: Duration,
    next_socket_try: Instant,
    next_presence_poll: Instant,
    game: Option<GamePresence>,
    live_task: Option<TaskHandle<()>>,
    next_live_poll: Instant,
    report: ConnectionReport,
    sent_report: Option<ConnectionReport>,
}

/// The intent dispatch + supervision loop.
pub async fn run(mut intents: IntentRx, sink: UpdateSink, settings: Settings) {
    tracing::info!("worker started");
    let ctx = Arc::new(Ctx::new(sink, settings));
    tokio::spawn(tasks::load_content(ctx.clone()));

    let (presence_tx, mut presence_rx) = mpsc::unbounded_channel();
    let now = Instant::now();
    let mut sup = Supervisor {
        ctx,
        phase: Phase::Disconnected,
        status: None,
        next_check: now,
        backoff: Duration::from_secs(2),
        presence_tx,
        socket: None,
        socket_connected: false,
        socket_backoff: Duration::from_secs(1),
        next_socket_try: now,
        next_presence_poll: now,
        game: None,
        live_task: None,
        next_live_poll: now,
        report: ConnectionReport::default(),
        sent_report: None,
    };
    tokio::spawn(social::configure(sup.ctx.clone()));
    if sup.ctx.settings.lock().unwrap().check_for_updates {
        tokio::spawn(tasks::check_for_updates(sup.ctx.clone()));
    }

    let mut tick = tokio::time::interval(Duration::from_millis(500));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            intent = intents.recv() => match intent {
                None | Some(Intent::Shutdown) => break,
                Some(i) => sup.handle(i),
            },
            Some(ev) = presence_rx.recv() => sup.on_presence(ev),
            _ = tick.tick() => sup.tick().await,
        }
    }
    sup.teardown();
    tracing::info!("worker stopped");
}

impl Supervisor {
    fn handle(&mut self, intent: Intent) {
        if social::handle(&self.ctx, &intent) {
            return;
        }
        let ctx = self.ctx.clone();
        match intent {
            Intent::Ping(n) => ctx.sink.send(Update::Pong(n)),
            Intent::Bootstrap => {
                self.backoff = Duration::from_secs(2);
                self.next_check = Instant::now();
            }
            Intent::RefreshLiveMatch => self.next_live_poll = Instant::now(),
            Intent::ProbePartyMmr => {
                tokio::spawn(tasks::rescan_lobby(ctx));
            }
            Intent::LookupPlayer { name, tag } => {
                tokio::spawn(tasks::lookup(ctx, tasks::LookupQuery::RiotId { name, tag }));
            }
            Intent::LookupPuuid(p) => {
                tokio::spawn(tasks::lookup(ctx, tasks::LookupQuery::Puuid(p)));
            }
            Intent::CheckForUpdates => {
                tokio::spawn(tasks::check_for_updates(ctx));
            }
            Intent::RefreshCareer => {
                tokio::spawn(tasks::refresh_career(ctx));
            }
            Intent::LoadMatchDetails(id) => {
                tokio::spawn(async move {
                    if let Err(e) = tasks::load_match(&ctx, &id).await {
                        ctx.report(&e);
                    }
                });
            }
            Intent::SaveSettings(s) => {
                // Update the in-memory copy now so intents queued right after
                // (e.g. BackendConfigure) see the new values; only the disk
                // write is deferred.
                *ctx.settings.lock().unwrap() = s.clone();
                tokio::spawn(tasks::save_settings(ctx, s));
            }
            Intent::Shutdown => {}
            // Backend intents are handled by `social::handle` above.
            _ => {}
        }
    }

    fn set_status(&mut self, s: ClientStatus) {
        if self.status.as_ref() != Some(&s) {
            self.status = Some(s.clone());
            self.ctx.sink.send(Update::ClientStatus(s));
        }
    }

    async fn tick(&mut self) {
        let now = Instant::now();
        if now >= self.next_check {
            match self.phase {
                Phase::Disconnected => self.try_connect().await,
                Phase::Authed { pid, port } => self.check_alive(pid, port).await,
            }
        }
        if let Phase::Authed { .. } = self.phase {
            self.maintain_presence().await;
            self.maintain_live();
        }
    }

    /// Poll GLZ every few seconds while in agent select / in game, and only
    /// slowly while in menus.
    fn maintain_live(&mut self) {
        let now = Instant::now();
        let busy = self.live_task.as_ref().is_some_and(|h| !h.is_finished());
        if busy || now < self.next_live_poll {
            return;
        }
        let state = self.game.as_ref().map(|g| g.loop_state).unwrap_or_default();
        let interval = if state.is_live() {
            self.ctx.settings.lock().unwrap().poll_interval()
        } else {
            MENU_LIVE_POLL
        };
        self.next_live_poll = now + interval;
        self.live_task = Some(tokio::spawn(tasks::refresh_live(self.ctx.clone(), state)));
    }

    fn publish_report(&mut self) {
        if self.sent_report.as_ref() != Some(&self.report) {
            self.sent_report = Some(self.report.clone());
            self.ctx.sink.send(Update::Connection(self.report.clone()));
        }
    }

    fn step(&mut self, step: Step, state: StepState) {
        if let StepState::Failed { message, .. } = &state {
            tracing::warn!(step = step.title(), "connection step failed: {message}");
        }
        self.report.set(step, state);
        self.publish_report();
    }

    /// Run the connection checklist one step at a time, reporting each.
    async fn try_connect(&mut self) {
        let now = Instant::now();
        let previous = self.report.clone();
        self.report.restart();

        self.report.set(Step::Lockfile, StepState::Running);
        let lock = match lockfile::read().await {
            Ok(l) => l,
            Err(e) => {
                let (status, state) = match &e {
                    ApiError::NotRunning => {
                        let path = lockfile::default_path()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        (
                            ClientStatus::NotRunning,
                            StepState::failed(
                                format!("The Riot Client isn't running (no lockfile at {path})"),
                                "Start the Riot Client and launch VALORANT. This page updates automatically.",
                            ),
                        )
                    }
                    other => (
                        ClientStatus::Connecting(UserFacingError::from(other).0),
                        StepState::failed(other.to_string(), "Restart the Riot Client."),
                    ),
                };
                self.set_status(status);
                // Polling every few seconds while nothing runs: don't bump the
                // attempt counter or re-send an identical report.
                self.report = ConnectionReport {
                    attempt: previous.attempt,
                    ..Default::default()
                };
                self.report.set(Step::Lockfile, state);
                self.publish_report();
                self.next_check = now + LOCKFILE_POLL;
                return;
            }
        };
        self.step(
            Step::Lockfile,
            StepState::Ok(format!(
                "Found the Riot Client on port {} (pid {})",
                lock.port, lock.pid
            )),
        );
        if !matches!(self.status, Some(ClientStatus::Connecting(_))) {
            self.set_status(ClientStatus::Connecting("Connecting to the Riot client…".into()));
        }

        match self.connect_steps(&lock).await {
            Ok(session) => {
                let session = Arc::new(session);
                self.set_status(ClientStatus::Authed {
                    riot_id: session.riot_id.clone(),
                    region: session.region.region.clone(),
                });
                if let Some(id) = &session.riot_id {
                    self.ctx.names.insert(&session.puuid, id.clone());
                }
                *self.ctx.session.write().unwrap() = Some(session);
                self.phase = Phase::Authed {
                    pid: lock.pid,
                    port: lock.port,
                };
                self.backoff = Duration::from_secs(2);
                self.next_check = now + ALIVE_CHECK;
                self.next_socket_try = now;
                self.next_presence_poll = now;
                self.step(Step::LiveUpdates, StepState::Running);
                tokio::spawn(tasks::seed_friend_names(self.ctx.clone()));
                tokio::spawn(tasks::refresh_career(self.ctx.clone()));
            }
            Err(msg) => {
                // Typical while the client is still starting; retry with backoff.
                tracing::info!("riot client not ready: {msg}");
                self.set_status(ClientStatus::Connecting(msg));
                self.next_check = now + self.backoff;
                self.backoff = (self.backoff * 2).min(MAX_BACKOFF);
            }
        }
    }

    /// Steps 2-6. On failure the failed step is already reported; the
    /// returned string is the short status-bar text.
    async fn connect_steps(&mut self, lock: &lockfile::Lockfile) -> Result<Session, String> {
        // Local API + login.
        self.step(Step::LocalApi, StepState::Running);
        let local = match LocalClient::new(lock) {
            Ok(l) => l,
            Err(e) => {
                self.step(
                    Step::LocalApi,
                    StepState::failed(e.to_string(), "This is a bug; please send the diagnostics."),
                );
                return Err("Couldn't connect to the Riot Client".into());
            }
        };
        let ent = match local.entitlements().await {
            Ok(e) => e,
            Err(e) => {
                let (msg, hint, status) = match &e {
                    ApiError::NotFound => (
                        "The Riot Client is running but you're not signed in".to_string(),
                        "Sign in to the Riot Client.",
                        "Sign in to the Riot Client",
                    ),
                    ApiError::Unauthorized => (
                        "The Riot Client rejected the lockfile password".to_string(),
                        "The client probably restarted; this retries automatically. If it keeps failing, restart the Riot Client.",
                        "Waiting for the Riot Client…",
                    ),
                    ApiError::Network(detail) => (
                        format!("Couldn't reach the Riot Client's local API: {detail}"),
                        "The client may still be starting. If this doesn't clear up, check that security software isn't blocking connections to 127.0.0.1, then copy the diagnostics.",
                        "Waiting for the Riot Client…",
                    ),
                    other => (
                        format!("The Riot Client's local API returned an error: {other}"),
                        "Restart the Riot Client. If it keeps failing, copy the diagnostics.",
                        "Waiting for the Riot Client…",
                    ),
                };
                self.step(Step::LocalApi, StepState::failed(msg, hint));
                return Err(status.into());
            }
        };
        let short = ent.subject.get(..8).unwrap_or(&ent.subject).to_string();
        self.step(Step::LocalApi, StepState::Ok(format!("Signed in (account {short}…)")));

        // Region.
        self.step(Step::Region, StepState::Running);
        let region = match riot::detect_region_with_source(&local).await {
            Ok((region, source)) => {
                let from = match source {
                    riot::RegionSource::LaunchArgs => "from VALORANT",
                    riot::RegionSource::ClientRegion => "from the Riot Client",
                    riot::RegionSource::ChatSession => "from your chat session",
                };
                self.step(
                    Step::Region,
                    StepState::Ok(format!(
                        "{} (shard {}), {from}",
                        region.region.to_uppercase(),
                        region.shard
                    )),
                );
                region
            }
            Err(_) => {
                self.step(
                    Step::Region,
                    StepState::failed(
                        "Couldn't work out your VALORANT region",
                        "Launch VALORANT itself; the Riot Client on its own doesn't always report a region.",
                    ),
                );
                return Err("Waiting for VALORANT to start…".into());
            }
        };

        // Client version (needed for every Riot server call).
        self.step(Step::ClientVersion, StepState::Running);
        let version = match self.ctx.client_version().await {
            Ok(v) => {
                self.step(Step::ClientVersion, StepState::Ok(v.clone()));
                v
            }
            Err(e) => {
                self.step(
                    Step::ClientVersion,
                    StepState::failed(
                        format!("Couldn't get the current game version: {e}"),
                        "Check your internet connection (the version comes from valorant-api.com, or VALORANT's own log once the game has started).",
                    ),
                );
                return Err("Couldn't get the game version".into());
            }
        };

        // Riot ID (nice to have).
        self.step(Step::Identity, StepState::Running);
        let riot_id = match local.chat_session().await {
            Ok(c) if !c.game_name.is_empty() => {
                let id = RiotId {
                    game_name: c.game_name,
                    tag_line: c.game_tag,
                };
                self.step(Step::Identity, StepState::Ok(id.to_string()));
                Some(id)
            }
            Ok(_) => {
                self.step(
                    Step::Identity,
                    StepState::Warn("Chat isn't ready yet; your Riot ID will show later".into()),
                );
                None
            }
            Err(e) => {
                self.step(
                    Step::Identity,
                    StepState::Warn(format!("Couldn't read your Riot ID: {e}")),
                );
                None
            }
        };

        let session = match Session::from_parts(local, ent, region, riot_id, self.ctx.http.clone(), version) {
            Ok(s) => s,
            Err(e) => {
                self.step(
                    Step::RiotServers,
                    StepState::failed(e.to_string(), "Please copy the diagnostics."),
                );
                return Err("Couldn't set up the Riot session".into());
            }
        };

        // A real authenticated call, so token or header problems show up here
        // rather than as an empty career screen.
        self.step(Step::RiotServers, StepState::Running);
        match pd::match_history(&session, &session.puuid, 0, 1).await {
            Ok(h) => {
                self.step(
                    Step::RiotServers,
                    StepState::Ok(format!("Connected ({} matches on record)", h.total)),
                );
                Ok(session)
            }
            Err(e) => {
                let hint = match &e {
                    ApiError::Unauthorized | ApiError::Status(400) | ApiError::Status(403) => {
                        "Riot rejected the session. Restart VALORANT; if it persists, the game version may be out of date, so copy the diagnostics."
                    }
                    ApiError::RateLimited(_) => "Riot is rate-limiting requests; this retries automatically.",
                    ApiError::Network(_) => "Check your internet connection or firewall.",
                    _ => "This retries automatically. If it keeps failing, copy the diagnostics.",
                };
                self.step(
                    Step::RiotServers,
                    StepState::failed(format!("Riot's match servers returned an error: {e}"), hint),
                );
                Err("Couldn't reach Riot's servers".into())
            }
        }
    }

    async fn check_alive(&mut self, pid: u32, port: u16) {
        let still_there = matches!(lockfile::read().await, Ok(l) if l.pid == pid && l.port == port);
        if still_there {
            self.next_check = Instant::now() + ALIVE_CHECK;
        } else {
            tracing::info!("riot client went away");
            self.teardown();
            self.phase = Phase::Disconnected;
            self.next_check = Instant::now();
            self.set_status(ClientStatus::NotRunning);
        }
    }

    /// Keep the websocket up; poll presence while it's down.
    async fn maintain_presence(&mut self) {
        let now = Instant::now();
        let Ok(session) = self.ctx.session() else { return };
        let socket_dead = self.socket.as_ref().is_none_or(|h| h.is_finished());
        if socket_dead && now >= self.next_socket_try {
            self.socket = Some(tokio::spawn(presence::run_socket(
                session.local.port(),
                session.local.password().clone(),
                session.puuid.clone(),
                self.presence_tx.clone(),
            )));
            self.next_socket_try = now + self.socket_backoff;
            self.socket_backoff = (self.socket_backoff * 2).min(MAX_BACKOFF);
        }
        if !self.socket_connected && now >= self.next_presence_poll {
            self.next_presence_poll = now + PRESENCE_POLL;
            if let Ok(p) = session.local.presences().await {
                if let Some(gp) = presence::own_presence(&p.presences, &session.puuid) {
                    self.on_presence(PresenceEvent::Changed(gp));
                }
            }
        }
    }

    fn on_presence(&mut self, ev: PresenceEvent) {
        match ev {
            PresenceEvent::Connected => {
                self.socket_connected = true;
                self.socket_backoff = Duration::from_secs(1);
                self.step(
                    Step::LiveUpdates,
                    StepState::Ok("Connected to the Riot Client's event stream".into()),
                );
            }
            PresenceEvent::Disconnected(why) => {
                self.socket_connected = false;
                if matches!(self.phase, Phase::Authed { .. }) {
                    self.step(
                        Step::LiveUpdates,
                        StepState::Warn(format!(
                            "Event stream unavailable, checking every few seconds instead ({why})"
                        )),
                    );
                }
            }
            PresenceEvent::Changed(gp) => {
                if self.game.as_ref() == Some(&gp) {
                    return;
                }
                let prev = self.game.as_ref().map(|g| g.loop_state).unwrap_or_default();
                tracing::info!(from = ?prev, to = ?gp.loop_state, "game state changed");
                // A match just ended: refresh career data.
                if prev == LoopState::InGame && gp.loop_state == LoopState::Menus {
                    tokio::spawn(tasks::refresh_career(self.ctx.clone()));
                }
                if prev != gp.loop_state || self.game.as_ref().map(|g| g.party_size) != Some(gp.party_size) {
                    self.next_live_poll = Instant::now();
                }
                self.game = Some(gp.clone());
                *self.ctx.game.lock().unwrap() = Some(gp.clone());
                social::publish_presence(&self.ctx, &gp);
                self.ctx.sink.send(Update::Game(Some(gp)));
            }
        }
    }

    fn teardown(&mut self) {
        if let Some(h) = self.socket.take() {
            h.abort();
        }
        if let Some(h) = self.live_task.take() {
            h.abort();
        }
        *self.ctx.live.lock().unwrap() = None;
        self.ctx.sink.send(Update::LiveMatch(None));
        self.socket_connected = false;
        self.game = None;
        *self.ctx.game.lock().unwrap() = None;
        *self.ctx.session.write().unwrap() = None;
        self.ctx.sink.send(Update::Game(None));
    }
}
