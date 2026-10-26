//! The async routines behind each [`Intent`]. Each one reports its results
//! (or a user-facing error) through the [`UpdateSink`].

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::backend::social_ws::ClientMsg;
use crate::backend::BackendClient;
use crate::config::Settings;
use crate::content::{valorant_api, Catalog};
use crate::error::{ApiError, UserFacingError};
use crate::ipc::{CareerSummary, PlayerCard, RrPoint, Update, UpdateSink, UpdateStatus};
use crate::riot::models::{LiveMatch, MatchDetail, MatchId, Puuid};
use crate::riot::names::NameCache;
use crate::riot::pd::PlayerProbe;
use crate::riot::presence::{GamePresence, LoopState};
use crate::riot::{account, glz, pd, RiotId, Session};

/// How many recent matches the career screen loads details for.
pub const CAREER_MATCHES: u32 = 10;
/// How many recent matches a lookup loads.
pub const LOOKUP_MATCHES: u32 = 5;
/// Rank probes are reused for this long (ranks change at most once a game).
pub const PROBE_TTL: Duration = Duration::from_secs(10 * 60);

/// Shared worker context. Lives in an `Arc` and is cloned into every task.
pub struct Ctx {
    pub sink: UpdateSink,
    pub http: reqwest::Client,
    pub session: RwLock<Option<Arc<Session>>>,
    pub catalog: RwLock<Arc<Catalog>>,
    pub names: NameCache,
    pub matches: Mutex<HashMap<MatchId, Arc<MatchDetail>>>,
    pub settings: Mutex<Settings>,
    pub cache_dir: Option<PathBuf>,
    pub probes: Mutex<HashMap<Puuid, (Instant, PlayerProbe)>>,
    pub probing: Mutex<HashSet<Puuid>>,
    pub live: Mutex<Option<LiveMatch>>,
    last_live_error: Mutex<Option<String>>,
    /// Our latest presence (menus / agent select / in game).
    pub game: Mutex<Option<GamePresence>>,
    /// Optional backend client (Phase 3); `None` in tracker-only mode.
    pub backend: RwLock<Option<Arc<BackendClient>>>,
    pub social_out: Mutex<Option<tokio::sync::mpsc::UnboundedSender<ClientMsg>>>,
    pub social_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Ctx {
    pub fn new(sink: UpdateSink, settings: Settings) -> Self {
        Self {
            sink,
            http: crate::riot::public_http(),
            session: RwLock::new(None),
            catalog: RwLock::new(Arc::new(Catalog::default())),
            names: NameCache::default(),
            matches: Mutex::new(HashMap::new()),
            settings: Mutex::new(settings),
            cache_dir: crate::config::cache_dir(),
            probes: Mutex::new(HashMap::new()),
            probing: Mutex::new(HashSet::new()),
            live: Mutex::new(None),
            last_live_error: Mutex::new(None),
            game: Mutex::new(None),
            backend: RwLock::new(None),
            social_out: Mutex::new(None),
            social_task: Mutex::new(None),
        }
    }

    pub fn session(&self) -> Result<Arc<Session>, ApiError> {
        self.session.read().unwrap().clone().ok_or(ApiError::NotRunning)
    }

    pub fn game_state(&self) -> Option<GamePresence> {
        self.game.lock().unwrap().clone()
    }

    pub fn catalog(&self) -> Arc<Catalog> {
        self.catalog.read().unwrap().clone()
    }

    pub fn set_catalog(&self, c: Catalog) {
        let c = Arc::new(c);
        *self.catalog.write().unwrap() = c.clone();
        self.sink.send(Update::Content(c));
    }

    pub fn report(&self, e: &ApiError) {
        tracing::warn!("{e}");
        self.sink.send(Update::Error(UserFacingError::from(e)));
    }

    /// Riot client version: env override, catalog, valorant-api.com, then
    /// the "CI server version" line in VALORANT's own log file.
    pub async fn client_version(&self) -> Result<String, ApiError> {
        if let Ok(v) = std::env::var("ASCEND_CLIENT_VERSION") {
            return Ok(v);
        }
        if let Some(v) = self.catalog().client_version.clone() {
            return Ok(v);
        }
        match valorant_api::client_version(&self.http).await {
            Ok(v) => Ok(v),
            Err(e) => crate::riot::client_version_from_log().await.ok_or(e),
        }
    }
}

/// Load valorant-api content, publish it, then cache icons and republish.
pub async fn load_content(ctx: Arc<Ctx>) {
    match valorant_api::fetch_catalog(&ctx.http).await {
        Ok(c) => {
            ctx.set_catalog(c.clone());
            if let Some(dir) = &ctx.cache_dir {
                let cached = valorant_api::cache_icons(&ctx.http, c, &dir.join("img")).await;
                ctx.set_catalog(cached);
            }
        }
        Err(e) => {
            tracing::warn!("couldn't load valorant-api content: {e}");
            ctx.sink.send(Update::Error(UserFacingError(
                "Couldn't load agent/map artwork (valorant-api.com)".into(),
            )));
        }
    }
}

/// Self rank, RR history and recent matches.
pub async fn refresh_career(ctx: Arc<Ctx>) {
    ctx.sink.send(Update::CareerLoading(true));
    if let Err(e) = career_inner(&ctx).await {
        ctx.report(&e);
    }
    ctx.sink.send(Update::CareerLoading(false));
}

async fn career_inner(ctx: &Arc<Ctx>) -> Result<(), ApiError> {
    let s = ctx.session()?;
    let act = ctx.catalog().current_act.clone();
    let (mmr, updates, history) = tokio::join!(
        pd::player_mmr(&s, &s.puuid),
        pd::competitive_updates(&s, &s.puuid, 20),
        pd::match_history(&s, &s.puuid, 0, CAREER_MATCHES),
    );
    let history = history?;
    let rank = match mmr {
        Ok(m) => pd::rank_snapshot(&m, act.as_deref()),
        Err(ApiError::NotFound) => Default::default(),
        Err(e) => return Err(e),
    };
    let mut rr_history: Vec<RrPoint> = updates
        .map(|u| u.matches)
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m.tier_after_update > 0)
        .map(|m| RrPoint {
            at_unix: m.match_start_time / 1000,
            tier: m.tier_after_update,
            rr: m.ranked_rating_after_update,
            earned: m.ranked_rating_earned,
        })
        .collect();
    rr_history.reverse();

    ctx.sink.send(Update::Career(CareerSummary {
        puuid: s.puuid.clone(),
        riot_id: s.riot_id.clone(),
        rank,
        rr_history,
        matches: history.history.clone(),
    }));

    // Details two at a time, each published as it lands.
    let sem = Arc::new(tokio::sync::Semaphore::new(2));
    let mut tasks = Vec::new();
    for entry in history.history {
        let (ctx, sem) = (ctx.clone(), sem.clone());
        tasks.push(tokio::spawn(async move {
            let _p = sem.acquire().await;
            if let Err(e) = load_match(&ctx, &entry.match_id).await {
                tracing::debug!(match_id = %entry.match_id, "match details failed: {e}");
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
    Ok(())
}

/// Load one match (memory cache -> disk cache -> Riot) and publish it.
pub async fn load_match(ctx: &Arc<Ctx>, id: &str) -> Result<Arc<MatchDetail>, ApiError> {
    if let Some(m) = ctx.matches.lock().unwrap().get(id).cloned() {
        ctx.sink.send(Update::MatchDetails(id.to_string(), m.clone()));
        return Ok(m);
    }
    let disk = ctx
        .cache_dir
        .as_ref()
        .map(|d| d.join("matches").join(format!("{id}.json")));
    let mut detail: Option<MatchDetail> = None;
    if let Some(path) = &disk {
        if let Ok(bytes) = tokio::fs::read(path).await {
            detail = serde_json::from_slice(&bytes).ok();
        }
    }
    let detail = match detail {
        Some(d) => d,
        None => {
            let s = ctx.session()?;
            let d = pd::match_details(&s, id).await?;
            // Finished matches never change, so they are safe to cache.
            if let (Some(path), true) = (&disk, d.info.is_completed) {
                if let Some(parent) = path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                if let Ok(json) = serde_json::to_vec(&d) {
                    let _ = tokio::fs::write(path, json).await;
                }
            }
            d
        }
    };
    for p in &detail.players {
        ctx.names.insert(
            &p.puuid,
            RiotId {
                game_name: p.game_name.clone(),
                tag_line: p.tag_line.clone(),
            },
        );
    }
    let detail = Arc::new(detail);
    ctx.matches.lock().unwrap().insert(id.to_string(), detail.clone());
    ctx.sink.send(Update::MatchDetails(id.to_string(), detail.clone()));
    Ok(detail)
}

/// Seed the name cache with the friend list so lookups can find friends.
pub async fn seed_friend_names(ctx: Arc<Ctx>) {
    let Ok(s) = ctx.session() else { return };
    if let Ok(f) = s.local.friends().await {
        for fr in f.friends {
            ctx.names.insert(
                &fr.puuid,
                RiotId {
                    game_name: fr.game_name,
                    tag_line: fr.game_tag,
                },
            );
        }
    }
}

pub async fn save_settings(ctx: Arc<Ctx>, settings: Settings) {
    *ctx.settings.lock().unwrap() = settings.clone();
    let res = tokio::task::spawn_blocking(move || crate::config::save(&settings)).await;
    if !matches!(res, Ok(Ok(()))) {
        ctx.sink
            .send(Update::Error(UserFacingError("Couldn't save settings".into())));
    }
}

/// Poll party / pre-game / core-game and probe anyone new.
pub async fn refresh_live(ctx: Arc<Ctx>, hint: LoopState) {
    let result = match ctx.session() {
        Ok(s) => glz::current(&s, hint).await,
        Err(e) => Err(e),
    };
    match result {
        Ok(live) => {
            *ctx.last_live_error.lock().unwrap() = None;
            let changed = *ctx.live.lock().unwrap() != live;
            if changed {
                *ctx.live.lock().unwrap() = live.clone();
                ctx.sink.send(Update::LiveMatch(live.clone()));
                super::social::publish_lobby(&ctx, live.as_ref());
            }
            if let Some(m) = live {
                probe_players(&ctx, &m, false).await;
            }
        }
        Err(e) => {
            // Live polling runs every few seconds: only surface new errors.
            let msg = e.to_string();
            let mut last = ctx.last_live_error.lock().unwrap();
            if last.as_deref() != Some(msg.as_str()) {
                *last = Some(msg);
                drop(last);
                ctx.report(&e);
            }
        }
    }
}

/// "Rescan lobby": forget cached probes for the current roster and redo them.
pub async fn rescan_lobby(ctx: Arc<Ctx>) {
    let live = ctx.live.lock().unwrap().clone();
    match live {
        Some(m) => {
            {
                let mut probes = ctx.probes.lock().unwrap();
                for p in &m.players {
                    probes.remove(&p.puuid);
                }
            }
            probe_players(&ctx, &m, true).await;
        }
        None => refresh_live(ctx, LoopState::Unknown).await,
    }
}

async fn probe_players(ctx: &Arc<Ctx>, m: &LiveMatch, force: bool) {
    let Ok(s) = ctx.session() else { return };
    // Names only for players who haven't enabled streamer mode.
    let visible: Vec<String> = m
        .players
        .iter()
        .filter(|p| !p.incognito)
        .map(|p| p.puuid.clone())
        .collect();
    match ctx.names.resolve(&s, &visible).await {
        Ok(names) => ctx.sink.send(Update::Names(names.into_iter().collect())),
        Err(e) => tracing::debug!("name lookup failed: {e}"),
    }
    let act = ctx.catalog().current_act.clone();
    let mut tasks = Vec::new();
    for p in &m.players {
        let puuid = p.puuid.clone();
        let cached = ctx
            .probes
            .lock()
            .unwrap()
            .get(&puuid)
            .filter(|(at, _)| at.elapsed() < PROBE_TTL)
            .cloned();
        if let (Some((_, probe)), false) = (cached, force) {
            ctx.sink.send(Update::Probe(puuid, Ok(probe)));
            continue;
        }
        if !ctx.probing.lock().unwrap().insert(puuid.clone()) {
            continue; // already in flight
        }
        let (ctx, s, act) = (ctx.clone(), s.clone(), act.clone());
        tasks.push(tokio::spawn(async move {
            let res = pd::probe(&s, &puuid, act.as_deref()).await;
            ctx.probing.lock().unwrap().remove(&puuid);
            match res {
                Ok(probe) => {
                    ctx.probes
                        .lock()
                        .unwrap()
                        .insert(puuid.clone(), (Instant::now(), probe.clone()));
                    ctx.sink.send(Update::Probe(puuid, Ok(probe)));
                }
                Err(e) => ctx.sink.send(Update::Probe(puuid, Err(UserFacingError::from(&e)))),
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
}

pub enum LookupQuery {
    RiotId { name: String, tag: String },
    Puuid(String),
}

pub async fn lookup(ctx: Arc<Ctx>, q: LookupQuery) {
    let res = lookup_inner(&ctx, q).await;
    match res {
        Ok((card, history)) => {
            ctx.sink.send(Update::PlayerLookup(Ok(card)));
            for entry in history {
                if let Err(e) = load_match(&ctx, &entry).await {
                    tracing::debug!("lookup match failed: {e}");
                }
            }
        }
        Err(e) => {
            let msg = match e {
                ApiError::NotFound => UserFacingError("No player found with that Riot ID".into()),
                other => UserFacingError::from(&other),
            };
            ctx.sink.send(Update::PlayerLookup(Err(msg)));
        }
    }
}

async fn lookup_inner(ctx: &Arc<Ctx>, q: LookupQuery) -> Result<(PlayerCard, Vec<MatchId>), ApiError> {
    let s = ctx.session()?;
    let puuid = match q {
        LookupQuery::Puuid(p) => p,
        LookupQuery::RiotId { name, tag } => match ctx.names.find(&name, &tag) {
            Some(p) => p,
            None => match account::api_key() {
                Some(key) => account::puuid_by_riot_id(&ctx.http, &s.region.region, &name, &tag, &key).await?,
                None => {
                    return Err(ApiError::Other(
                        "Player not seen yet. Lookups work for friends, recent teammates/opponents and \
                         lobby players, or set RIOT_API_KEY to search anyone."
                            .into(),
                    ))
                }
            },
        },
    };
    if !pd::is_uuid(&puuid) {
        return Err(ApiError::Other("invalid player id".into()));
    }
    let act = ctx.catalog().current_act.clone();
    let (probe, history, names) = tokio::join!(
        pd::probe(&s, &puuid, act.as_deref()),
        pd::match_history(&s, &puuid, 0, LOOKUP_MATCHES),
        ctx.names.resolve(&s, std::slice::from_ref(&puuid)),
    );
    let history = history?;
    let card = PlayerCard {
        riot_id: names.ok().and_then(|n| n.get(&puuid).cloned()),
        puuid,
        probe: probe?,
        matches: history.history.clone(),
    };
    Ok((card, history.history.into_iter().map(|h| h.match_id).collect()))
}

/// Check GitHub releases (or any JSON with `tag_name`/`html_url`).
pub async fn check_for_updates(ctx: Arc<Ctx>) {
    ctx.sink.send(Update::UpdateStatus(UpdateStatus::Checking));
    let url = ctx.settings.lock().unwrap().update_url.clone();
    let status = match ctx
        .http
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(r) if r.status().as_u16() == 404 => UpdateStatus::NoReleases,
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
            Ok(v) => compare_release(&v, env!("CARGO_PKG_VERSION")),
            Err(e) => UpdateStatus::Failed(e.to_string()),
        },
        Ok(r) => UpdateStatus::Failed(format!("HTTP {}", r.status().as_u16())),
        Err(e) => UpdateStatus::Failed(e.without_url().to_string()),
    };
    ctx.sink.send(Update::UpdateStatus(status));
}

pub fn compare_release(release: &serde_json::Value, current: &str) -> UpdateStatus {
    let tag = release.get("tag_name").and_then(|v| v.as_str()).unwrap_or_default();
    let url = release
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    match (
        semver::Version::parse(tag.trim_start_matches('v')),
        semver::Version::parse(current),
    ) {
        (Ok(latest), Ok(cur)) if latest > cur => UpdateStatus::Available {
            version: latest.to_string(),
            url,
        },
        (Ok(_), Ok(_)) => UpdateStatus::UpToDate,
        _ => UpdateStatus::Failed(format!("unrecognised release tag {tag:?}")),
    }
}
