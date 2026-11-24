//! Accolades, agent mastery, play-session recaps and per-map stats, built
//! from loaded match details. Pure functions, so they're easy to test.
//!
//! Everything here is derived from Riot's match details. Accolades are
//! Tracked's own; agent mastery follows VALORANT's published Agent Mastery
//! rates and thresholds. The rules are documented next to each item so the
//! numbers can be explained in the UI.

use std::collections::HashMap;

use crate::ipc::RrPoint;
use crate::riot::models::{MatchDetail, Outcome};
use crate::stats::{performance_scores, Totals};

// --------------------------------------------------------------- accolades

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Accolade {
    /// Highest performance score in the lobby.
    MatchMvp,
    /// Highest performance score on your team (when not match MVP).
    TeamMvp,
    /// Killed all five enemies in one round.
    Ace,
    /// Won a 1vX clutch.
    Clutch,
    /// Four kills in one round.
    QuadKill,
    /// Three kills in one round.
    TripleKill,
    /// Most kills in the lobby.
    TopFragger,
    /// Most damage per round in the lobby.
    DamageDealer,
    /// Most first kills in the lobby (at least 3), with more first kills
    /// than first deaths.
    EntryKing,
    /// Headshot rate of 35%+ over at least 20 hits.
    Headhunter,
    /// KAST of 85%+: you mattered almost every round.
    Unshakeable,
    /// Four or more spike plants + defuses.
    SpikeSpecialist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rarity {
    Common,
    Rare,
    Epic,
    Legendary,
}

impl Accolade {
    pub const ALL: [Accolade; 12] = [
        Accolade::MatchMvp,
        Accolade::TeamMvp,
        Accolade::Ace,
        Accolade::Clutch,
        Accolade::QuadKill,
        Accolade::TripleKill,
        Accolade::TopFragger,
        Accolade::DamageDealer,
        Accolade::EntryKing,
        Accolade::Headhunter,
        Accolade::Unshakeable,
        Accolade::SpikeSpecialist,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Accolade::MatchMvp => "Match MVP",
            Accolade::TeamMvp => "Team MVP",
            Accolade::Ace => "Ace",
            Accolade::Clutch => "Clutch Master",
            Accolade::QuadKill => "Quadra Kill",
            Accolade::TripleKill => "Triple Kill",
            Accolade::TopFragger => "Top Fragger",
            Accolade::DamageDealer => "Damage Dealer",
            Accolade::EntryKing => "Entry King",
            Accolade::Headhunter => "Headhunter",
            Accolade::Unshakeable => "Unshakeable",
            Accolade::SpikeSpecialist => "Spike Specialist",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Accolade::MatchMvp => "Highest performance score in the match",
            Accolade::TeamMvp => "Highest performance score on your team",
            Accolade::Ace => "Killed all five enemies in a single round",
            Accolade::Clutch => "Won a round as the last player standing",
            Accolade::QuadKill => "Four kills in a single round",
            Accolade::TripleKill => "Three kills in a single round",
            Accolade::TopFragger => "Most kills in the match",
            Accolade::DamageDealer => "Most damage per round in the match",
            Accolade::EntryKing => "Most opening kills in the match (3+)",
            Accolade::Headhunter => "35%+ headshot rate over 20+ hits",
            Accolade::Unshakeable => "KAST of 85% or more",
            Accolade::SpikeSpecialist => "Four or more plants and defuses",
        }
    }

    pub fn rarity(self) -> Rarity {
        match self {
            Accolade::Ace => Rarity::Legendary,
            Accolade::MatchMvp | Accolade::QuadKill | Accolade::Clutch => Rarity::Epic,
            Accolade::EntryKing | Accolade::Headhunter | Accolade::Unshakeable | Accolade::SpikeSpecialist => {
                Rarity::Rare
            }
            _ => Rarity::Common,
        }
    }

    /// Mastery points this accolade adds on top of the match's base points.
    pub fn points(self) -> u32 {
        match self.rarity() {
            Rarity::Common => 20,
            Rarity::Rare => 35,
            Rarity::Epic => 60,
            Rarity::Legendary => 120,
        }
    }
}

/// The accolades `puuid` earned in one match (empty if they weren't in it).
pub fn accolades(m: &MatchDetail, puuid: &str) -> Vec<Accolade> {
    let Some(me) = m.player(puuid) else {
        return Vec::new();
    };
    let s = &me.stats;
    let mut out = Vec::new();
    let scores = performance_scores(m);
    let perf = |p: &str| scores.iter().find(|(id, _)| id == p).map(|(_, s)| *s);
    if let Some(mine) = perf(puuid) {
        let best = scores.iter().map(|(_, s)| *s).max().unwrap_or(0);
        let team_best = m
            .players
            .iter()
            .filter(|p| p.team_id == me.team_id)
            .filter_map(|p| perf(&p.puuid))
            .max()
            .unwrap_or(0);
        if mine == best {
            out.push(Accolade::MatchMvp);
        } else if mine == team_best {
            out.push(Accolade::TeamMvp);
        }
    }
    if s.aces > 0 {
        out.push(Accolade::Ace);
    }
    if s.clutches_won > 0 {
        out.push(Accolade::Clutch);
    }
    if s.multi_kill_rounds4 > 0 {
        out.push(Accolade::QuadKill);
    } else if s.multi_kill_rounds3 > 0 {
        out.push(Accolade::TripleKill);
    }
    let others = || m.players.iter().filter(|p| p.puuid != puuid);
    if s.kills > 0 && others().all(|p| p.stats.kills <= s.kills) {
        out.push(Accolade::TopFragger);
    }
    if s.rounds_played > 0 && others().all(|p| p.stats.adr() <= s.adr()) {
        out.push(Accolade::DamageDealer);
    }
    if s.first_kills >= 3 && s.first_kills > s.first_deaths && others().all(|p| p.stats.first_kills <= s.first_kills) {
        out.push(Accolade::EntryKing);
    }
    if s.headshots + s.bodyshots + s.legshots >= 20 && s.hs_pct() >= 35.0 {
        out.push(Accolade::Headhunter);
    }
    if s.rounds_played >= 10 && s.kast_pct() >= 85.0 {
        out.push(Accolade::Unshakeable);
    }
    if s.plants + s.defuses >= 4 {
        out.push(Accolade::SpikeSpecialist);
    }
    out
}

/// How often each accolade was earned, with the most recent match it came
/// from. Every accolade is listed, earned or not, in [`Accolade::ALL`] order.
#[derive(Debug, Clone, PartialEq)]
pub struct AccoladeCount {
    pub accolade: Accolade,
    pub count: u32,
    /// (match id, start unix seconds) of the latest time it was earned.
    pub last: Option<(String, i64)>,
}

pub fn accolade_counts(puuid: &str, matches: &[&MatchDetail]) -> Vec<AccoladeCount> {
    let mut map: HashMap<Accolade, (u32, Option<(String, i64)>)> = HashMap::new();
    for m in matches {
        for a in accolades(m, puuid) {
            let e = map.entry(a).or_default();
            e.0 += 1;
            if e.1.as_ref().is_none_or(|(_, t)| m.info.started_at_unix > *t) {
                e.1 = Some((m.info.match_id.clone(), m.info.started_at_unix));
            }
        }
    }
    Accolade::ALL
        .iter()
        .map(|a| {
            let (count, last) = map.remove(a).unwrap_or_default();
            AccoladeCount {
                accolade: *a,
                count,
                last,
            }
        })
        .collect()
}

/// One accolade earned in one match, for the "recently earned" feed.
#[derive(Debug, Clone, PartialEq)]
pub struct EarnedAccolade {
    pub accolade: Accolade,
    pub match_id: String,
    pub at: i64,
    pub map_id: String,
    pub agent_id: String,
}

/// Every accolade earned, newest first, rarest first within a match.
pub fn accolade_feed(puuid: &str, matches: &[&MatchDetail]) -> Vec<EarnedAccolade> {
    let mut out = Vec::new();
    for m in matches {
        let Some(me) = m.player(puuid) else { continue };
        let mut list = accolades(m, puuid);
        list.sort_by_key(|a| std::cmp::Reverse(a.rarity()));
        for a in list {
            out.push(EarnedAccolade {
                accolade: a,
                match_id: m.info.match_id.clone(),
                at: m.info.started_at_unix,
                map_id: m.info.map_id.clone(),
                agent_id: me.character_id.clone(),
            });
        }
    }
    out.sort_by(|a, b| b.at.cmp(&a.at).then(b.accolade.rarity().cmp(&a.accolade.rarity())));
    out
}

// ---------------------------------------------------------------- mastery
//
// VALORANT's own Agent Mastery (Patch 13.06): every agent has an Act track
// (10 levels per act, overlevelling allowed) and a Lifetime track. Mastery
// Points (MP) are earned per minute played: 104/min for a win, 80/min for a
// loss and up to 120/min for the match MVP in Competitive. Progress started
// from zero at the patch, so earlier games don't count.
//
// Riot doesn't expose mastery through any API Tracked can read, so this
// rebuilds it from the matches Tracked has loaded with Riot's published
// rates and thresholds. It will be lower than the game if some of your
// games this act aren't loaded.

/// When Patch 13.06 went live (Agent Mastery started from zero then).
pub const MASTERY_START_UNIX: i64 = 1_790_121_600; // 2026-09-23 00:00 UTC

/// MP per minute.
pub const MP_WIN: u32 = 104;
pub const MP_LOSS: u32 = 80;
pub const MP_MVP: u32 = 120;

/// Total MP to reach each Act level, from Riot's published milestones
/// (1, 2, 4, 7 and 10); levels in between are interpolated.
pub const ACT_LEVELS: [u32; 10] = [
    2_000, 10_500, 20_400, 30_300, 44_167, 58_033, 71_900, 92_067, 112_233, 132_400,
];

/// MP per level after level 10 (the last published step).
const OVERLEVEL_STEP: u32 = (132_400 - 71_900) / 3;

/// Act levels with a reward: a coloured border at 4, portrait background
/// upgrades at 7 and 10.
pub const ACT_MILESTONES: [u32; 3] = [4, 7, 10];

/// Modes that earn MP at the win/loss rates. Other modes earn the loss rate
/// for their duration.
fn full_mode(queue: &str) -> bool {
    matches!(
        queue,
        "competitive" | "unrated" | "swiftplay" | "spikerush" | "hurm" | "premier"
    )
}

/// Mastery points for one match.
pub fn match_points(m: &MatchDetail, puuid: &str) -> u32 {
    if m.player(puuid).is_none() || m.info.started_at_unix < MASTERY_START_UNIX {
        return 0;
    }
    let minutes = (m.info.game_length_ms as f32 / 60_000.0).max(0.0);
    let queue = m.info.queue_id.as_str();
    let rate = if !full_mode(queue) {
        MP_LOSS
    } else if queue == "competitive" && accolades(m, puuid).contains(&Accolade::MatchMvp) {
        MP_MVP
    } else if matches!(m.outcome_for(puuid), Some((Outcome::Win, ..))) {
        MP_WIN
    } else {
        MP_LOSS
    };
    (minutes * rate as f32).round() as u32
}

/// Total MP needed to reach `level` (0 for level 0).
pub fn points_for_level(level: u32) -> u32 {
    match level {
        0 => 0,
        1..=10 => ACT_LEVELS[level as usize - 1],
        _ => ACT_LEVELS[9] + (level - 10) * OVERLEVEL_STEP,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    /// Act level, starting at 0.
    pub level: u32,
    /// MP into the current level, and MP the next level needs.
    pub into: u32,
    pub needed: u32,
}

impl Level {
    pub fn from_points(points: u32) -> Level {
        let mut level = 0;
        while points >= points_for_level(level + 1) {
            level += 1;
        }
        let base = points_for_level(level);
        Level {
            level,
            into: points - base,
            needed: points_for_level(level + 1) - base,
        }
    }

    pub fn progress(&self) -> f32 {
        self.into as f32 / self.needed.max(1) as f32
    }

    /// The last reward reached ("Level 7 reward"), or "" below level 4.
    pub fn tier(&self) -> String {
        ACT_MILESTONES
            .iter()
            .rev()
            .find(|&&l| self.level >= l)
            .map(|l| format!("Level {l} reward"))
            .unwrap_or_default()
    }

    /// 0 below level 4, then 1 (border), 2 (level 7), 3 (level 10+).
    pub fn tier_index(&self) -> usize {
        ACT_MILESTONES.iter().filter(|&&l| self.level >= l).count()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentMastery {
    pub agent_id: String,
    pub totals: Totals,
    /// Lifetime MP (since Agent Mastery started).
    pub points: u32,
    /// MP this act, which sets the Act level.
    pub act_points: u32,
    pub level: Level,
    pub accolades: u32,
    /// (performance score, match id) of the best game on this agent.
    pub best: Option<(u32, String)>,
    /// Map id -> (wins, games) on this agent.
    pub maps: Vec<(String, u32, u32)>,
}

/// Mastery for every agent played, highest Act MP first. The act is the one
/// of the newest loaded match.
pub fn mastery(puuid: &str, matches: &[&MatchDetail]) -> Vec<AgentMastery> {
    let act = matches
        .iter()
        .max_by_key(|m| m.info.started_at_unix)
        .map(|m| m.info.season_id.clone())
        .unwrap_or_default();
    let mut map: HashMap<String, AgentMastery> = HashMap::new();
    let mut agent_maps: HashMap<String, HashMap<String, (u32, u32)>> = HashMap::new();
    for m in matches {
        let Some(me) = m.player(puuid) else { continue };
        let perf = crate::stats::performance(m, puuid);
        let e = map.entry(me.character_id.clone()).or_insert_with(|| AgentMastery {
            agent_id: me.character_id.clone(),
            totals: Totals::default(),
            points: 0,
            act_points: 0,
            level: Level::from_points(0),
            accolades: 0,
            best: None,
            maps: Vec::new(),
        });
        e.totals.add_match(m, puuid, perf);
        let mp = match_points(m, puuid);
        e.points += mp;
        if m.info.season_id == act {
            e.act_points += mp;
        }
        e.accolades += accolades(m, puuid).len() as u32;
        if let Some(p) = perf {
            if e.best.as_ref().is_none_or(|(b, _)| p > *b) {
                e.best = Some((p, m.info.match_id.clone()));
            }
        }
        let won = matches!(m.outcome_for(puuid), Some((Outcome::Win, ..)));
        let slot = agent_maps
            .entry(me.character_id.clone())
            .or_default()
            .entry(m.info.map_id.clone())
            .or_default();
        slot.0 += won as u32;
        slot.1 += 1;
    }
    let mut v: Vec<AgentMastery> = map
        .into_values()
        .map(|mut a| {
            a.level = Level::from_points(a.act_points);
            let mut maps: Vec<(String, u32, u32)> = agent_maps
                .remove(&a.agent_id)
                .unwrap_or_default()
                .into_iter()
                .map(|(k, (w, g))| (k, w, g))
                .collect();
            maps.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0)));
            a.maps = maps;
            a
        })
        .collect();
    v.sort_by(|a, b| {
        (b.act_points, b.points, b.totals.matches)
            .cmp(&(a.act_points, a.points, a.totals.matches))
            .then(a.agent_id.cmp(&b.agent_id))
    });
    v
}

// ---------------------------------------------------------------- sessions

/// A gap longer than this between two matches starts a new play session.
pub const SESSION_GAP_SECS: i64 = 3 * 3600;

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// Start of the first match and end of the last (unix seconds).
    pub start: i64,
    pub end: i64,
    /// Match ids, oldest first.
    pub matches: Vec<String>,
    pub totals: Totals,
    /// Net ranked rating over the session's competitive games, when known.
    pub rr: Option<i32>,
    pub accolades: Vec<Accolade>,
    /// Performance score per match, oldest first.
    pub perf: Vec<u32>,
    /// Agent id -> games.
    pub agents: Vec<(String, u32)>,
    /// (performance, match id) of the session's best game.
    pub best: Option<(u32, String)>,
}

/// Group matches into play sessions, newest session first.
pub fn sessions(puuid: &str, matches: &[&MatchDetail], rr_history: &[RrPoint]) -> Vec<Session> {
    let mut ms: Vec<&&MatchDetail> = matches.iter().filter(|m| m.player(puuid).is_some()).collect();
    ms.sort_by_key(|m| m.info.started_at_unix);
    let rr: HashMap<&str, i32> = rr_history.iter().map(|p| (p.match_id.as_str(), p.earned)).collect();
    let mut out: Vec<Session> = Vec::new();
    let mut last_end = i64::MIN;
    for m in ms {
        let start = m.info.started_at_unix;
        let end = start + (m.info.game_length_ms / 1000) as i64;
        if out.is_empty() || start - last_end > SESSION_GAP_SECS {
            out.push(Session {
                start,
                end,
                matches: Vec::new(),
                totals: Totals::default(),
                rr: None,
                accolades: Vec::new(),
                perf: Vec::new(),
                agents: Vec::new(),
                best: None,
            });
        }
        last_end = last_end.max(end);
        let s = out.last_mut().unwrap();
        s.end = s.end.max(end);
        s.matches.push(m.info.match_id.clone());
        let perf = crate::stats::performance(m, puuid);
        s.totals.add_match(m, puuid, perf);
        if let Some(p) = perf {
            s.perf.push(p);
            if s.best.as_ref().is_none_or(|(b, _)| p > *b) {
                s.best = Some((p, m.info.match_id.clone()));
            }
        }
        if let Some(e) = rr.get(m.info.match_id.as_str()) {
            *s.rr.get_or_insert(0) += e;
        }
        s.accolades.extend(accolades(m, puuid));
        let agent = &m.player(puuid).unwrap().character_id;
        match s.agents.iter_mut().find(|(a, _)| a == agent) {
            Some(e) => e.1 += 1,
            None => s.agents.push((agent.clone(), 1)),
        }
    }
    for s in &mut out {
        s.agents.sort_by_key(|a| std::cmp::Reverse(a.1));
        s.accolades.sort_by_key(|a| std::cmp::Reverse(a.rarity()));
    }
    out.reverse();
    out
}

// -------------------------------------------------------------------- maps

#[derive(Debug, Clone, PartialEq)]
pub struct MapStats {
    pub map_id: String,
    pub totals: Totals,
}

/// Per-map totals, most played first.
pub fn maps(puuid: &str, matches: &[&MatchDetail]) -> Vec<MapStats> {
    let mut map: HashMap<String, Totals> = HashMap::new();
    for m in matches {
        if m.player(puuid).is_none() {
            continue;
        }
        map.entry(m.info.map_id.clone())
            .or_default()
            .add_match(m, puuid, crate::stats::performance(m, puuid));
    }
    let mut v: Vec<MapStats> = map
        .into_iter()
        .map(|(map_id, totals)| MapStats { map_id, totals })
        .collect();
    v.sort_by(|a, b| b.totals.matches.cmp(&a.totals.matches).then(a.map_id.cmp(&b.map_id)));
    v
}

/// Current win/loss streak over matches (newest first): (+n wins / -n losses).
pub fn streak(puuid: &str, matches: &[&MatchDetail]) -> i32 {
    let mut ms: Vec<&&MatchDetail> = matches.iter().filter(|m| m.player(puuid).is_some()).collect();
    ms.sort_by_key(|m| std::cmp::Reverse(m.info.started_at_unix));
    let mut n = 0i32;
    for m in ms {
        match m.outcome_for(puuid) {
            Some((Outcome::Win, ..)) if n >= 0 => n += 1,
            Some((Outcome::Loss, ..)) if n <= 0 => n -= 1,
            _ => break,
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::riot::models::{MatchInfo, PlayerMatch, PlayerStats, TeamResult};

    fn stats(kills: u32, aces: u32) -> PlayerStats {
        PlayerStats {
            kills,
            deaths: 10,
            score: 200 * 20,
            rounds_played: 20,
            damage_made: 150 * kills,
            headshots: 10,
            bodyshots: 10,
            kast_rounds: 18,
            aces,
            ..Default::default()
        }
    }

    fn m(id: &str, t: i64, me_kills: u32, won: bool) -> MatchDetail {
        let player = |p: &str, team: &str, s: PlayerStats| PlayerMatch {
            puuid: p.into(),
            team_id: team.into(),
            character_id: "jett".into(),
            stats: s,
            ..Default::default()
        };
        MatchDetail {
            schema: MatchDetail::SCHEMA,
            info: MatchInfo {
                match_id: id.into(),
                map_id: "ascent".into(),
                started_at_unix: t,
                game_length_ms: 30 * 60_000,
                ..Default::default()
            },
            players: vec![
                player("me", "A", stats(me_kills, 1)),
                player("mate", "A", stats(10, 0)),
                player("x", "B", stats(12, 0)),
                player("y", "B", stats(8, 0)),
            ],
            teams: vec![
                TeamResult {
                    team_id: "A".into(),
                    won,
                    rounds_won: if won { 13 } else { 7 },
                    rounds_played: 20,
                },
                TeamResult {
                    team_id: "B".into(),
                    won: !won,
                    rounds_won: if won { 7 } else { 13 },
                    rounds_played: 20,
                },
            ],
            rounds: vec![],
        }
    }

    #[test]
    fn accolades_for_a_carry_game() {
        let g = m("1", 0, 25, true);
        let a = accolades(&g, "me");
        assert!(a.contains(&Accolade::MatchMvp));
        assert!(a.contains(&Accolade::Ace));
        assert!(a.contains(&Accolade::TopFragger));
        assert!(a.contains(&Accolade::DamageDealer));
        assert!(a.contains(&Accolade::Headhunter), "50% HS over 20 hits");
        assert!(a.contains(&Accolade::Unshakeable), "90% KAST");
        assert!(!a.contains(&Accolade::TeamMvp), "MVP replaces team MVP");
        assert!(accolades(&g, "nobody").is_empty());
    }

    #[test]
    fn mastery_levels() {
        assert_eq!(Level::from_points(0).level, 0);
        assert_eq!(Level::from_points(1_999).level, 0);
        let l = Level::from_points(10_500 + 100);
        assert_eq!((l.level, l.into, l.needed), (2, 100, 20_400 - 10_500));
        assert_eq!(Level::from_points(132_400).level, 10);
        assert_eq!(Level::from_points(132_400 + OVERLEVEL_STEP).level, 11);
        assert_eq!(Level::from_points(30_300).tier(), "Level 4 reward");
        assert_eq!(Level::from_points(0).tier_index(), 0);

        let t0 = MASTERY_START_UNIX + 3600;
        let mut games = [m("1", t0, 25, true), m("2", t0 + 4000, 5, false), m("old", 0, 30, true)];
        for g in &mut games {
            g.info.queue_id = "unrated".into();
            g.info.season_id = "act".into();
        }
        games[0].info.season_id = "earlier-act".into();
        let refs: Vec<&MatchDetail> = games.iter().collect();
        // 30-minute games: a win is 104/min, a loss 80/min, pre-patch games count nothing.
        assert_eq!(match_points(&games[0], "me"), 3_120);
        assert_eq!(match_points(&games[1], "me"), 2_400);
        assert_eq!(match_points(&games[2], "me"), 0);
        let ms = mastery("me", &refs);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].totals.matches, 3);
        assert_eq!(ms[0].points, 5_520, "lifetime");
        assert_eq!(ms[0].act_points, 2_400, "only the newest match's act");
        assert_eq!(ms[0].level.level, 1);

        games[0].info.queue_id = "competitive".into();
        assert_eq!(match_points(&games[0], "me"), 3_600, "match MVP in competitive");
    }

    #[test]
    fn sessions_split_on_gaps() {
        let h = 3600;
        let games = [m("1", 0, 20, true), m("2", h, 10, false), m("3", 10 * h, 15, true)];
        let refs: Vec<&MatchDetail> = games.iter().collect();
        let rr = vec![RrPoint {
            match_id: "1".into(),
            earned: 21,
            ..Default::default()
        }];
        let s = sessions("me", &refs, &rr);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].matches, vec!["3".to_string()], "newest session first");
        assert_eq!(s[1].matches.len(), 2);
        assert_eq!((s[1].totals.wins, s[1].totals.losses), (1, 1));
        assert_eq!(s[1].rr, Some(21));
        assert_eq!(s[0].rr, None);
        assert_eq!(streak("me", &refs), 1);
        assert_eq!(maps("me", &refs)[0].totals.matches, 3);
        let counts = accolade_counts("me", &refs);
        assert_eq!(counts.len(), Accolade::ALL.len());
        assert_eq!(counts.iter().find(|c| c.accolade == Accolade::Ace).unwrap().count, 3);
        assert_eq!(accolade_feed("me", &refs)[0].match_id, "3");
    }
}
