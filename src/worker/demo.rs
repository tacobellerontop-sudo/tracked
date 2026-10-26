//! `--demo` mode: answers intents with synthetic data so the UI can be
//! explored (and screenshotted) without a Riot client. Never contacts Riot;
//! it still loads public artwork from valorant-api.com when reachable.

use std::sync::Arc;

use serde_json::json;

use super::tasks::{self, Ctx};
use crate::content::valorant_api::{Agent, MapInfo, Tier};
use crate::content::Catalog;
use crate::ipc::{CareerSummary, ClientStatus, Intent, IntentRx, PlayerCard, RrPoint, Update, UpdateStatus};
use crate::riot::match_stats::normalize;
use crate::riot::models::{LiveMatch, LivePhase, LivePlayer, MatchDetail, MatchHistoryEntry, RawMatch};
use crate::riot::pd::{PlayerProbe, RankSnapshot};
use crate::riot::presence::{GamePresence, LoopState};
use crate::riot::RiotId;

pub const SELF_PUUID: &str = "00000000-0000-4000-8000-000000000000";
const AGENTS: &[&str] = &[
    "Jett", "Sova", "Omen", "Killjoy", "Raze", "Skye", "Viper", "Reyna", "Cypher", "Breach",
];
const MAPS: &[&str] = &["Ascent", "Bind", "Haven", "Split", "Lotus", "Sunset", "Icebox", "Pearl"];
const NAMES: &[&str] = &[
    "you", "Nimbus", "kettle", "Orchid", "zeroday", "Paprika", "lowtide", "Quasar", "brick", "Vesper",
];
const TIER_NAMES: &[&str] = &[
    "Iron",
    "Bronze",
    "Silver",
    "Gold",
    "Platinum",
    "Diamond",
    "Ascendant",
    "Immortal",
];

/// Tiny deterministic PRNG so the demo looks the same every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

/// "Now" for demo timestamps, so relative times ("2h ago") look right.
fn demo_now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn puuid(i: usize) -> String {
    format!("00000000-0000-4000-8000-{i:012}")
}

fn agent_id(i: usize) -> String {
    format!("a0000000-0000-4000-8000-{i:012}")
}

pub fn catalog() -> Catalog {
    let mut c = Catalog::default();
    for (i, a) in AGENTS.iter().enumerate() {
        c.agents.insert(
            agent_id(i),
            Agent {
                name: a.to_string(),
                icon: String::new(),
            },
        );
    }
    for m in MAPS {
        c.maps.insert(
            format!("/Game/Maps/{m}/{m}"),
            MapInfo {
                name: m.to_string(),
                icon: String::new(),
                splash: String::new(),
            },
        );
    }
    for (i, t) in TIER_NAMES.iter().enumerate() {
        for d in 1..=3 {
            c.tiers.insert(
                3 + i as u32 * 3 + d - 1,
                Tier {
                    name: format!("{t} {d}"),
                    ..Default::default()
                },
            );
        }
    }
    c.tiers.insert(
        27,
        Tier {
            name: "Radiant".into(),
            ..Default::default()
        },
    );
    c
}

/// A plausible 5v5 competitive match built from raw kills/damage, so the
/// demo also exercises the real stat normalization.
pub fn synthetic_match(seed: u64) -> MatchDetail {
    let mut rng = Lcg(seed);
    let teams = ["Red", "Blue"];
    let players: Vec<_> = (0..10)
        .map(|i| {
            json!({ "subject": puuid(i), "gameName": NAMES[i], "tagLine": format!("{}", 1000 + i * 7),
                    "teamId": teams[i / 5], "characterId": agent_id((i + seed as usize) % AGENTS.len()),
                    "competitiveTier": 12 + rng.next(6) as u32, "accountLevel": 40 + rng.next(300) })
        })
        .collect();
    let (mut red, mut blue) = (0, 0);
    let mut rounds = Vec::new();
    let mut score = [0u32; 10];
    let mut kda = [(0u32, 0u32, 0u32); 10];
    let mut n = 0;
    while red < 13 && blue < 13 {
        let mut alive: Vec<Vec<usize>> = vec![(0..5).collect(), (5..10).collect()];
        let mut stats: Vec<serde_json::Value> = Vec::new();
        let mut t = 5000;
        while !alive[0].is_empty() && !alive[1].is_empty() {
            // Slight edge to the red team so the demo user wins more often.
            let killer_side = if rng.next(100) < 53 { 0 } else { 1 };
            let killer = alive[killer_side][rng.next(alive[killer_side].len() as u64) as usize];
            let vi = rng.next(alive[1 - killer_side].len() as u64) as usize;
            let victim = alive[1 - killer_side].remove(vi);
            t += 1000 + rng.next(9000);
            let hs = rng.next(3) as u32;
            let assist: Vec<String> = alive[killer_side]
                .iter()
                .filter(|&&p| p != killer && rng.next(3) == 0)
                .map(|&p| puuid(p))
                .collect();
            for a in &assist {
                let idx: usize = a[a.len() - 2..].parse().unwrap();
                kda[idx].2 += 1;
            }
            kda[killer].0 += 1;
            kda[victim].1 += 1;
            score[killer] += 150 + 50 * hs;
            stats.push(json!({ "subject": puuid(killer),
                "kills": [{ "roundTime": t, "killer": puuid(killer), "victim": puuid(victim), "assistants": assist }],
                "damage": [{ "receiver": puuid(victim), "damage": 100 + rng.next(60), "headshots": hs, "bodyshots": 2 - hs.min(2), "legshots": rng.next(2) }] }));
        }
        let winner = if alive[0].is_empty() { 1 } else { 0 };
        if winner == 0 {
            red += 1
        } else {
            blue += 1
        }
        let attacker = if n < 12 { 0 } else { 1 };
        rounds.push(json!({ "roundNum": n, "winningTeam": teams[winner],
            "winningTeamRole": if winner == attacker { "Attacker" } else { "Defender" },
            "roundResult": "Eliminated", "playerStats": stats }));
        n += 1;
    }
    let players: Vec<_> = players
        .into_iter()
        .enumerate()
        .map(|(i, mut p)| {
            p["stats"] = json!({ "score": score[i], "roundsPlayed": n, "kills": kda[i].0, "deaths": kda[i].1, "assists": kda[i].2 });
            p
        })
        .collect();
    let raw: RawMatch = serde_json::from_value(json!({
        "matchInfo": { "matchId": format!("d{seed:07}-0000-4000-8000-000000000000"), "mapId": format!("/Game/Maps/{0}/{0}", MAPS[seed as usize % MAPS.len()]),
                       "queueID": "competitive", "gameLengthMillis": 1_500_000 + seed * 37_000, "gameStartMillis": demo_now_ms() - 2 * 3_600_000 - (seed as i64 - 1) * 26 * 3_600_000,
                       "isRanked": true, "isCompleted": true },
        "players": players,
        "teams": [{ "teamId": "Red", "won": red > blue, "roundsWon": red, "roundsPlayed": n },
                  { "teamId": "Blue", "won": blue > red, "roundsWon": blue, "roundsPlayed": n }],
        "roundResults": rounds
    }))
    .expect("demo match");
    normalize(&raw)
}

/// Demo player 7 has streamer mode on.
const DEMO_INCOGNITO: usize = 7;

fn live_match() -> LiveMatch {
    LiveMatch {
        phase: LivePhase::InGame,
        match_id: "demo".into(),
        map_id: "/Game/Maps/Lotus/Lotus".into(),
        queue_id: "competitive".into(),
        server: "aresriot.aws-rclusterprod-euc1-1.eu-gp-frankfurt-1".into(),
        players: (0..10)
            .map(|i| LivePlayer {
                puuid: puuid(i),
                team_id: if i < 5 { "Red" } else { "Blue" }.into(),
                character_id: agent_id(i),
                locked: true,
                account_level: (i != DEMO_INCOGNITO).then_some(40 + i as u32 * 23),
                incognito: i == DEMO_INCOGNITO,
                is_self: i == 0,
            })
            .collect(),
    }
}

fn probe(i: usize) -> PlayerProbe {
    let tier = [16, 15, 17, 14, 18, 16, 19, 15, 13, 17][i];
    let games = 20 + i as u32 * 3;
    PlayerProbe {
        rank: RankSnapshot {
            tier,
            rr: (i as i32 * 17) % 100,
            season_wins: games / 2 + (i as u32 % 4),
            season_games: games,
            peak_tier: tier + 2,
        },
        recent_rr: (0..5)
            .map(|k| if (i + k).is_multiple_of(3) { -14 } else { 19 })
            .collect(),
    }
}

fn send_live(ctx: &Ctx) {
    let game = GamePresence {
        loop_state: LoopState::InGame,
        party_size: 2,
        queue_id: "competitive".into(),
        map: "/Game/Maps/Lotus/Lotus".into(),
        ..Default::default()
    };
    *ctx.game.lock().unwrap() = Some(game.clone());
    ctx.sink.send(Update::Game(Some(game)));
    ctx.sink.send(Update::LiveMatch(Some(live_match())));
    ctx.sink.send(Update::Names(
        (0..10)
            .filter(|i| *i != DEMO_INCOGNITO)
            .map(|i| {
                (
                    puuid(i),
                    RiotId {
                        game_name: NAMES[i].into(),
                        tag_line: format!("{}", 1000 + i * 7),
                    },
                )
            })
            .collect(),
    ));
    for i in 0..10 {
        ctx.sink.send(Update::Probe(puuid(i), Ok(probe(i))));
    }
}

fn card(i: usize, matches: &[MatchDetail]) -> PlayerCard {
    PlayerCard {
        puuid: puuid(i),
        riot_id: Some(RiotId {
            game_name: NAMES[i].into(),
            tag_line: format!("{}", 1000 + i * 7),
        }),
        probe: probe(i),
        matches: matches[..5]
            .iter()
            .map(|m| MatchHistoryEntry {
                match_id: m.info.match_id.clone(),
                game_start_time: m.info.started_at_unix * 1000,
                queue_id: m.info.queue_id.clone(),
            })
            .collect(),
    }
}

pub async fn run(mut intents: IntentRx, ctx: Arc<Ctx>) {
    ctx.set_catalog(catalog());
    let mut report = crate::diagnostics::ConnectionReport::default();
    for (step, detail) in [
        (crate::diagnostics::Step::Lockfile, "Demo mode: no Riot Client needed"),
        (crate::diagnostics::Step::LocalApi, "Signed in (demo)"),
        (crate::diagnostics::Step::Region, "EU (shard eu)"),
        (crate::diagnostics::Step::ClientVersion, "demo"),
        (crate::diagnostics::Step::Identity, "you#1000"),
        (crate::diagnostics::Step::RiotServers, "Connected (synthetic data)"),
        (crate::diagnostics::Step::LiveUpdates, "Connected (synthetic data)"),
    ] {
        report.set(step, crate::diagnostics::StepState::Ok(detail.into()));
    }
    ctx.sink.send(Update::Connection(report));
    let me = RiotId {
        game_name: "you".into(),
        tag_line: "1000".into(),
    };
    ctx.sink.send(Update::ClientStatus(ClientStatus::Authed {
        riot_id: Some(me.clone()),
        region: "eu".into(),
    }));
    ctx.sink.send(Update::Game(Some(GamePresence {
        loop_state: LoopState::Menus,
        party_size: 2,
        queue_id: "competitive".into(),
        ..Default::default()
    })));
    let matches: Vec<MatchDetail> = (1..=20).map(synthetic_match).collect();
    let send_career = || {
        // A climb from Silver 3 to Platinum 2 over the last ~6 months.
        let now = demo_now_ms() / 1000;
        let mut ladder = 11 * 100 + 40;
        let mut rr_history: Vec<RrPoint> = (0..20)
            .map(|i| {
                let earned = [
                    28, -14, 31, 26, -12, 29, -9, 32, 27, -15, 30, 33, 24, -18, 35, 22, -11, 29, 26, 23,
                ][i];
                let before = ladder / 100;
                ladder += earned;
                let m = &matches[19 - i];
                RrPoint {
                    at_unix: now - (19 - i as i64) * 9 * 86_400 - 7200,
                    tier: (ladder / 100) as u32,
                    tier_before: before as u32,
                    rr: ladder % 100,
                    earned,
                    map_id: m.info.map_id.clone(),
                    match_id: m.info.match_id.clone(),
                }
            })
            .collect();
        // The most recent games are the ones in the match list.
        for (p, m) in rr_history.iter_mut().rev().zip(&matches) {
            p.at_unix = m.info.started_at_unix;
        }
        let last = rr_history.last().cloned().unwrap();
        ctx.sink.send(Update::Career(CareerSummary {
            puuid: puuid(0),
            riot_id: Some(me.clone()),
            profile: crate::ipc::Profile {
                account_level: Some(187),
                title: Some("Lobby Scout".into()),
                ..Default::default()
            },
            rank: RankSnapshot {
                tier: last.tier,
                rr: last.rr,
                season_wins: 23,
                season_games: 41,
                peak_tier: 18,
            },
            rr_history,
            matches: matches
                .iter()
                .map(|m| MatchHistoryEntry {
                    match_id: m.info.match_id.clone(),
                    game_start_time: m.info.started_at_unix * 1000,
                    queue_id: m.info.queue_id.clone(),
                })
                .collect(),
        }));
        for m in &matches {
            ctx.sink
                .send(Update::MatchDetails(m.info.match_id.clone(), Arc::new(m.clone())));
        }
    };
    send_career();

    send_live(&ctx);

    // With a backend configured, the demo signs straight in so the Social
    // screen has something to show.
    let social_ctx = ctx.clone();
    tokio::spawn(async move {
        super::social::configure(social_ctx.clone()).await;
        if social_ctx.backend.read().unwrap().is_some() {
            super::social::sign_in(social_ctx, "you".into()).await;
        }
    });
    while let Some(intent) = intents.recv().await {
        if super::social::handle(&ctx, &intent) {
            continue;
        }
        match intent {
            Intent::Ping(n) => ctx.sink.send(Update::Pong(n)),
            Intent::RefreshCareer => send_career(),
            Intent::RefreshLiveMatch | Intent::ProbePartyMmr => send_live(&ctx),
            Intent::CheckForUpdates => ctx.sink.send(Update::UpdateStatus(UpdateStatus::UpToDate)),
            Intent::LookupPuuid(p) => {
                let i = (0..10).find(|i| puuid(*i) == p).unwrap_or(1);
                ctx.sink.send(Update::PlayerLookup(Ok(card(i, &matches))));
            }
            Intent::LookupPlayer { name, .. } => {
                let found = (0..10).find(|i| NAMES[*i].eq_ignore_ascii_case(&name) && *i != DEMO_INCOGNITO);
                ctx.sink.send(Update::PlayerLookup(match found {
                    Some(i) => Ok(card(i, &matches)),
                    None => Err(crate::error::UserFacingError(
                        "No player found with that Riot ID".into(),
                    )),
                }));
            }
            Intent::SaveSettings(s) => tasks::save_settings(ctx.clone(), s).await,
            Intent::Shutdown => break,
            _ => {}
        }
    }
}
