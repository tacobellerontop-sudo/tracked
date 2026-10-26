//! The worker owns the tokio runtime and runs every piece of I/O.
//!
//! [`run`] is a single supervisor loop that:
//! * dispatches [`Intent`]s (spawning a task per piece of work),
//! * drives the connection lifecycle (brief §9):
//!   `NotRunning → SignedOut → Connecting → Ready (Authed) → Lost → NotRunning`,
//!   using [`riot::connect_observed`] and reporting every step to the startup
//!   checklist,
//! * consumes [`riot::watch`] for live game state.

pub mod demo;
pub mod friends;
pub mod social;
pub mod tasks;

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle as TaskHandle;
use tokio::time::Instant;

use crate::config::Settings;
use crate::diagnostics::{ConnectionReport, Step, StepState};
use crate::error::ApiError;
use crate::ipc::{ClientStatus, Intent, IntentRx, Update, UpdateSink};
use crate::riot::presence::{GamePresence, LoopState};
use crate::riot::{self, lockfile, pd, GameState, RiotError, WatchEvent};
use tasks::Ctx;

/// Lockfile / sign-in check cadence while not connected.
const LOCKFILE_POLL: Duration = Duration::from_millis(2500);
/// How often we confirm the client is still there once connected.
const ALIVE_CHECK: Duration = Duration::from_secs(3);
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
    /// Consecutive failed checks that the client's port is still open.
    dead_checks: u32,
    watch_tx: mpsc::UnboundedSender<WatchEvent>,
    watch_task: Option<TaskHandle<()>>,
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

    let (watch_tx, mut watch_rx) = mpsc::unbounded_channel();
    let now = Instant::now();
    let mut sup = Supervisor {
        ctx,
        phase: Phase::Disconnected,
        status: None,
        next_check: now,
        backoff: Duration::from_secs(2),
        dead_checks: 0,
        watch_tx,
        watch_task: None,
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
            Some(ev) = watch_rx.recv() => sup.on_watch(ev),
            _ = tick.tick() => sup.tick().await,
        }
    }
    sup.teardown();
    tracing::info!("worker stopped");
}

impl Supervisor {
    fn handle(&mut self, intent: Intent) {
        if social::handle(&self.ctx, &intent) || friends::handle(&self.ctx, &intent) {
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
            tracing::info!(status = ?s, "client status");
            self.status = Some(s.clone());
            self.ctx.sink.send(Update::ClientStatus(s));
        }
    }

    async fn tick(&mut self) {
        if Instant::now() >= self.next_check {
            match self.phase {
                Phase::Disconnected => self.try_connect().await,
                Phase::Authed { pid, port } => self.check_alive(pid, port).await,
            }
        }
        if let Phase::Authed { .. } = self.phase {
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
        self.report.set(step, state);
        self.publish_report();
    }

    /// One connection attempt through [`riot::connect_observed`], mirroring
    /// every step into the startup checklist.
    async fn try_connect(&mut self) {
        let now = Instant::now();
        self.report.attempt += 1;
        let http = self.ctx.http.clone();
        let result = {
            let (report, sent, sink) = (&mut self.report, &mut self.sent_report, self.ctx.sink.clone());
            let mut observe = |step: Step, state: StepState| {
                // Re-checking a step that failed last time: keep showing the
                // failure until it's resolved instead of flickering.
                if matches!(state, StepState::Running) && matches!(report.state(step), StepState::Failed { .. }) {
                    return;
                }
                if let StepState::Failed { message, .. } = &state {
                    tracing::info!(step = step.title(), "connection step failed: {message}");
                }
                report.set(step, state);
                if sent.as_ref() != Some(&*report) {
                    *sent = Some(report.clone());
                    sink.send(Update::Connection(report.clone()));
                }
            };
            riot::connect_observed(&http, &mut observe).await
        };

        let session = match result {
            Ok(s) => s,
            Err(e) => {
                self.reset_steps_after_failure();
                let (status, wait) = match &e {
                    RiotError::ClientNotRunning => (ClientStatus::NotRunning, LOCKFILE_POLL),
                    RiotError::SignedOut => (ClientStatus::SignedOut, LOCKFILE_POLL),
                    RiotError::RegionUnknown => (
                        ClientStatus::Connecting("Waiting for VALORANT to start…".into()),
                        self.bump_backoff(),
                    ),
                    other => (ClientStatus::Connecting(other.to_string()), self.bump_backoff()),
                };
                self.set_status(status);
                self.next_check = now + wait;
                return;
            }
        };
        self.set_status(ClientStatus::Connecting("Connecting…".into()));

        // M2: one real authenticated PD call, so token or header problems
        // show up here instead of as an empty career screen.
        self.step(Step::RiotServers, StepState::Running);
        match pd::player_mmr(&session, &session.puuid).await {
            Ok(_) => self.step(
                Step::RiotServers,
                StepState::Ok("Connected (player data answered)".into()),
            ),
            Err(ApiError::NotFound) => self.step(
                Step::RiotServers,
                StepState::Ok("Connected (no ranked data on this account yet)".into()),
            ),
            Err(e) => {
                let hint = match &e {
                    ApiError::Unauthorized | ApiError::Status(400) | ApiError::Status(403) => {
                        "Riot rejected the session. Restart VALORANT; if it persists, copy the diagnostics."
                    }
                    ApiError::RateLimited(_) => "Riot is rate-limiting requests; this retries automatically.",
                    ApiError::Network(_) => "Check your internet connection or firewall.",
                    _ => "This retries automatically. If it keeps failing, copy the diagnostics.",
                };
                self.step(
                    Step::RiotServers,
                    StepState::failed(format!("Riot's player-data server returned an error: {e}"), hint),
                );
                self.set_status(ClientStatus::Connecting("Couldn't reach Riot's servers".into()));
                self.next_check = now + self.bump_backoff();
                return;
            }
        }

        let session = Arc::new(session);
        self.set_status(ClientStatus::Authed {
            riot_id: Some(session.riot_id()),
            region: session.region.as_str().into(),
        });
        self.ctx.names.insert(&session.puuid, session.riot_id());
        *self.ctx.session.write().unwrap() = Some(session.clone());
        self.phase = Phase::Authed {
            pid: session.local.pid(),
            port: session.local.port(),
        };
        self.backoff = Duration::from_secs(2);
        self.dead_checks = 0;
        self.next_check = now + ALIVE_CHECK;
        self.next_live_poll = now;

        // M3: live game state.
        self.step(Step::LiveUpdates, StepState::Running);
        let tx = self.watch_tx.clone();
        self.watch_task = Some(tokio::spawn(async move {
            let mut events = std::pin::pin!(riot::watch(session));
            while let Some(ev) = events.next().await {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        }));
        tokio::spawn(friends::refresh(self.ctx.clone()));
        tokio::spawn(tasks::refresh_career(self.ctx.clone()));
    }

    fn bump_backoff(&mut self) -> Duration {
        let wait = self.backoff;
        self.backoff = (self.backoff * 2).min(MAX_BACKOFF);
        wait
    }

    /// After a failed attempt, steps past the failure haven't run: show them
    /// as pending rather than stale results from an earlier attempt.
    fn reset_steps_after_failure(&mut self) {
        let failed = self.report.failure().map(|f| f.0);
        if let Some(failed) = failed {
            let mut after = false;
            for step in Step::ALL {
                if after {
                    self.report.set(step, StepState::Pending);
                }
                after |= step == failed;
            }
            self.publish_report();
        }
    }

    /// Lost detection: the lockfile vanished/changed, or the client's port
    /// stopped accepting connections.
    async fn check_alive(&mut self, pid: u32, port: u16) {
        let lock_ok = matches!(lockfile::read().await, Ok(l) if l.pid == pid && l.port == port);
        let port_ok = lock_ok
            && tokio::time::timeout(
                Duration::from_secs(1),
                tokio::net::TcpStream::connect(("127.0.0.1", port)),
            )
            .await
            .is_ok_and(|r| r.is_ok());
        self.dead_checks = if port_ok { 0 } else { self.dead_checks + 1 };
        // A vanished lockfile is definitive; a closed port gets one more try.
        if !lock_ok || self.dead_checks >= 2 {
            tracing::info!(lock_ok, "riot client went away");
            self.set_status(ClientStatus::Lost);
            self.teardown();
            self.phase = Phase::Disconnected;
            self.report = ConnectionReport {
                attempt: self.report.attempt,
                ..Default::default()
            };
            self.publish_report();
            self.next_check = Instant::now() + Duration::from_secs(1);
        } else {
            self.next_check = Instant::now() + ALIVE_CHECK;
        }
    }

    fn on_watch(&mut self, ev: WatchEvent) {
        if !matches!(self.phase, Phase::Authed { .. }) {
            return;
        }
        match ev {
            WatchEvent::SocketUp => self.step(
                Step::LiveUpdates,
                StepState::Ok("Connected to the Riot Client's event stream".into()),
            ),
            WatchEvent::SocketDown(why) => self.step(
                Step::LiveUpdates,
                StepState::Warn(format!(
                    "Event stream unavailable, checking every few seconds instead ({why})"
                )),
            ),
            WatchEvent::Presences(list) => {
                let me = self.ctx.session().map(|s| s.puuid.clone()).unwrap_or_default();
                self.ctx.sink.send(Update::RiotStatuses(friends::statuses(&list, &me)));
            }
            WatchEvent::Chat(msgs) => self.ctx.sink.send(Update::RiotChat(msgs)),
            WatchEvent::State { state, presence } => {
                if state == GameState::Offline {
                    // Maybe the client is closing: check right away.
                    self.next_check = Instant::now();
                }
                let gp = presence.or_else(|| {
                    (state != GameState::Offline).then(|| GamePresence {
                        loop_state: state.loop_state(),
                        ..Default::default()
                    })
                });
                if self.game == gp {
                    return;
                }
                let prev = self.game.as_ref().map(|g| g.loop_state).unwrap_or_default();
                let next = gp.as_ref().map(|g| g.loop_state).unwrap_or_default();
                tracing::info!(from = ?prev, to = ?next, state = ?state, "game state changed");
                // A match just ended: refresh career data.
                if prev == LoopState::InGame && next == LoopState::Menus {
                    tokio::spawn(tasks::refresh_career(self.ctx.clone()));
                }
                if prev != next || self.game.as_ref().map(|g| g.party_size) != gp.as_ref().map(|g| g.party_size) {
                    self.next_live_poll = Instant::now();
                }
                self.game = gp.clone();
                *self.ctx.game.lock().unwrap() = gp.clone();
                if let Some(g) = &gp {
                    social::publish_presence(&self.ctx, g);
                }
                self.ctx.sink.send(Update::Game(gp));
            }
        }
    }

    fn teardown(&mut self) {
        for h in [self.watch_task.take(), self.live_task.take()].into_iter().flatten() {
            h.abort();
        }
        self.game = None;
        *self.ctx.game.lock().unwrap() = None;
        *self.ctx.session.write().unwrap() = None;
        *self.ctx.live.lock().unwrap() = None;
        self.ctx.sink.send(Update::LiveMatch(None));
        self.ctx.sink.send(Update::Game(None));
    }
}
