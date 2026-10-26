//! The async routines behind each [`Intent`]. Each one reports its results
//! (or a user-facing error) through the [`UpdateSink`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crate::config::Settings;
use crate::content::{valorant_api, Catalog};
use crate::error::{ApiError, UserFacingError};
use crate::ipc::{CareerSummary, RrPoint, Update, UpdateSink};
use crate::riot::models::{MatchDetail, MatchId};
use crate::riot::names::NameCache;
use crate::riot::{pd, RiotId, Session};

/// How many recent matches the career screen loads details for.
pub const CAREER_MATCHES: u32 = 10;

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
        }
    }

    pub fn session(&self) -> Result<Arc<Session>, ApiError> {
        self.session.read().unwrap().clone().ok_or(ApiError::NotRunning)
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
