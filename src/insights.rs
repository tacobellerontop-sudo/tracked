//! Accolades, agent mastery, play-session recaps and per-map stats, built
//! from loaded match details. Pure functions, so they're easy to test.
//!
//! Everything here is derived from Riot's match details: nothing is a Riot
//! "official" mastery or accolade. The rules are documented next to each
//! item so the numbers can be explained in the UI.

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

/// Mastery tiers, every five levels.
pub const MASTERY_TIERS: [&str; 6] = ["Recruit", "Initiate", "Adept", "Veteran", "Expert", "Master"];

/// Points to go from `level` to `level + 1`: 300, 350, 400…
pub fn points_for_level(level: u32) -> u32 {
    300 + 50 * level.saturating_sub(1)
}

/// Mastery points for one match: 100 for playing, 60 for a win, up to 150
/// for performing above the lobby average, plus each accolade's points.
pub fn match_points(m: &MatchDetail, puuid: &str) -> u32 {
    let Some(_) = m.player(puuid) else { return 0 };
    let mut pts = 100;
    if matches!(m.outcome_for(puuid), Some((Outcome::Win, ..))) {
        pts += 60;
    }
    if let Some(perf) = crate::stats::performance(m, puuid) {
        pts += (perf.saturating_sub(500) * 3 / 10).min(150);
    }
    pts + accolades(m, puuid).iter().map(|a| a.points()).sum::<u32>()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    /// 1-based.
    pub level: u32,
    /// Points into the current level, and points the level needs.
    pub into: u32,
    pub needed: u32,
}

impl Level {
    pub fn from_points(mut points: u32) -> Level {
        let mut level = 1;
        while points >= points_for_level(level) {
            points -= points_for_level(level);
            level += 1;
        }
        Level {
            level,
            into: points,
            needed: points_for_level(level),
        }
    }

    pub fn progress(&self) -> f32 {
        self.into as f32 / self.needed.max(1) as f32
    }

    pub fn tier(&self) -> &'static str {
        MASTERY_TIERS[((self.level - 1) / 5).min(MASTERY_TIERS.len() as u32 - 1) as usize]
    }

    /// 0-based tier index (for colours).
    pub fn tier_index(&self) -> usize {
        ((self.level - 1) / 5).min(MASTERY_TIERS.len() as u32 - 1) as usize
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentMastery {
    pub agent_id: String,
    pub totals: Totals,
    pub points: u32,
    pub level: Level,
    pub accolades: u32,
    /// (performance score, match id) of the best game on this agent.
    pub best: Option<(u32, String)>,
    /// Map id -> (wins, games) on this agent.
    pub maps: Vec<(String, u32, u32)>,
}

/// Mastery for every agent played, highest points first.
pub fn mastery(puuid: &str, matches: &[&MatchDetail]) -> Vec<AgentMastery> {
    let mut map: HashMap<String, AgentMastery> = HashMap::new();
    let mut agent_maps: HashMap<String, HashMap<String, (u32, u32)>> = HashMap::new();
    for m in matches {
        let Some(me) = m.player(puuid) else { continue };
        let perf = crate::stats::performance(m, puuid);
        let e = map.entry(me.character_id.clone()).or_insert_with(|| AgentMastery {
            agent_id: me.character_id.clone(),
            totals: Totals::default(),
            points: 0,
            level: Level::from_points(0),
            accolades: 0,
            best: None,
            maps: Vec::new(),
        });
        e.totals.add_match(m, puuid, perf);
        e.points += match_points(m, puuid);
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
            a.level = Level::from_points(a.points);
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
    v.sort_by(|a, b| b.points.cmp(&a.points).then(a.agent_id.cmp(&b.agent_id)));
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
        assert_eq!(Level::from_points(0).level, 1);
        assert_eq!(Level::from_points(299).level, 1);
        let l = Level::from_points(300 + 350 + 10);
        assert_eq!((l.level, l.into, l.needed), (3, 10, 400));
        assert_eq!(Level::from_points(0).tier(), "Recruit");
        let games = [m("1", 0, 25, true), m("2", 4000, 5, false)];
        let refs: Vec<&MatchDetail> = games.iter().collect();
        let ms = mastery("me", &refs);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].totals.matches, 2);
        assert_eq!(ms[0].best.as_ref().unwrap().1, "1");
        assert!(ms[0].points > 260, "two games plus a win and accolades");
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
