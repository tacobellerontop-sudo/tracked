//! The worker owns the tokio runtime and runs every piece of I/O.
//!
//! [`run`] is a single supervisor loop that:
//! * dispatches [`Intent`]s (spawning a task per piece of work),
//! * drives the connection state machine (lockfile -> authed session),
//! * keeps the presence websocket alive, falling back to polling.

pub mod demo;
pub mod tasks;

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle as TaskHandle;
use tokio::time::Instant;

use crate::config::Settings;
use crate::error::{ApiError, UserFacingError};
use crate::ipc::{ClientStatus, Intent, IntentRx, Update, UpdateSink};
use crate::riot::presence::{self, GamePresence, LoopState, PresenceEvent};
use crate::riot::{lockfile, Session};
use tasks::Ctx;

/// Lockfile check cadence while the client isn't running.
const LOCKFILE_POLL: Duration = Duration::from_millis(2500);
/// How often we confirm the client is still there once authed.
const ALIVE_CHECK: Duration = Duration::from_secs(3);
/// Presence polling cadence when the websocket is down.
const PRESENCE_POLL: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

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
    };

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
        let ctx = self.ctx.clone();
        match intent {
            Intent::Ping(n) => ctx.sink.send(Update::Pong(n)),
            Intent::Bootstrap => {
                self.backoff = Duration::from_secs(2);
                self.next_check = Instant::now();
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
                tokio::spawn(tasks::save_settings(ctx, s));
            }
            Intent::Shutdown => {}
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
        }
    }

    async fn try_connect(&mut self) {
        let now = Instant::now();
        let lock = match lockfile::read().await {
            Ok(l) => l,
            Err(ApiError::NotRunning) => {
                self.set_status(ClientStatus::NotRunning);
                self.next_check = now + LOCKFILE_POLL;
                return;
            }
            Err(e) => {
                self.set_status(ClientStatus::Connecting(UserFacingError::from(&e).0));
                self.next_check = now + LOCKFILE_POLL;
                return;
            }
        };
        if !matches!(self.status, Some(ClientStatus::Connecting(_))) {
            self.set_status(ClientStatus::Connecting("Connecting to the Riot client…".into()));
        }
        let result = match self.ctx.client_version().await {
            Ok(version) => Session::connect(&lock, self.ctx.http.clone(), version).await,
            Err(e) => Err(e),
        };
        match result {
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
                tokio::spawn(tasks::seed_friend_names(self.ctx.clone()));
                tokio::spawn(tasks::refresh_career(self.ctx.clone()));
            }
            Err(e) => {
                // Typical while the client is still starting; retry with backoff.
                tracing::info!("riot client not ready: {e}");
                let msg = match e {
                    ApiError::RegionUnknown => "Waiting for VALORANT to start…".to_string(),
                    other => UserFacingError::from(&other).0,
                };
                self.set_status(ClientStatus::Connecting(msg));
                self.next_check = now + self.backoff;
                self.backoff = (self.backoff * 2).min(MAX_BACKOFF);
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
            }
            PresenceEvent::Disconnected => self.socket_connected = false,
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
                self.game = Some(gp.clone());
                self.ctx.sink.send(Update::Game(Some(gp)));
            }
        }
    }

    fn teardown(&mut self) {
        if let Some(h) = self.socket.take() {
            h.abort();
        }
        self.socket_connected = false;
        self.game = None;
        *self.ctx.session.write().unwrap() = None;
        self.ctx.sink.send(Update::Game(None));
    }
}
