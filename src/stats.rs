//! Aggregates for the Home dashboard, computed from loaded match details and
//! competitive updates. Pure functions, so they're easy to test.

use std::collections::HashMap;
use std::sync::Arc;

use crate::ipc::RrPoint;
use crate::riot::models::{MatchDetail, Outcome};

/// Totals over a set of matches, for one player.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Totals {
    pub matches: u32,
    pub wins: u32,
    pub losses: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub score: u32,
    pub rounds: u32,
    pub damage: u32,
    pub headshots: u32,
    pub hits: u32,
}

impl Totals {
    fn add(&mut self, s: &crate::riot::models::PlayerStats) {
        self.kills += s.kills;
        self.deaths += s.deaths;
        self.assists += s.assists;
        self.score += s.score;
        self.rounds += s.rounds_played;
        self.damage += s.damage_made;
        self.headshots += s.headshots;
        self.hits += s.headshots + s.bodyshots + s.legshots;
    }

    pub fn kd(&self) -> f32 {
        self.kills as f32 / self.deaths.max(1) as f32
    }

    pub fn acs(&self) -> f32 {
        self.score as f32 / self.rounds.max(1) as f32
    }

    pub fn adr(&self) -> f32 {
        self.damage as f32 / self.rounds.max(1) as f32
    }

    pub fn hs_pct(&self) -> f32 {
        100.0 * self.headshots as f32 / self.hits.max(1) as f32
    }

    /// Win rate over decided matches (draws excluded), 0-100.
    pub fn win_rate(&self) -> Option<f32> {
        let decided = self.wins + self.losses;
        (decided > 0).then(|| 100.0 * self.wins as f32 / decided as f32)
    }
}

/// The player's totals and, for comparison, the average player in the same
/// lobbies (everyone else in those matches).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Overview {
    pub me: Totals,
    pub lobby: Totals,
}

pub fn overview(puuid: &str, matches: &[&MatchDetail]) -> Overview {
    let mut o = Overview::default();
    for m in matches {
        let Some(me) = m.player(puuid) else { continue };
        o.me.matches += 1;
        o.me.add(&me.stats);
        match m.outcome_for(puuid) {
            Some((Outcome::Win, ..)) => o.me.wins += 1,
            Some((Outcome::Loss, ..)) => o.me.losses += 1,
            _ => {}
        }
        for p in m.players.iter().filter(|p| p.puuid != puuid) {
            o.lobby.add(&p.stats);
        }
    }
    o
}

/// Percentage difference of `value` relative to `baseline`.
pub fn relative_delta(value: f32, baseline: f32) -> Option<f32> {
    (baseline > 0.0).then(|| 100.0 * (value - baseline) / baseline)
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentStats {
    pub agent_id: String,
    pub totals: Totals,
}

/// Per-agent totals, most played first.
pub fn agents(puuid: &str, matches: &[&MatchDetail]) -> Vec<AgentStats> {
    let mut map: HashMap<String, Totals> = HashMap::new();
    for m in matches {
        let Some(me) = m.player(puuid) else { continue };
        let t = map.entry(me.character_id.clone()).or_default();
        t.matches += 1;
        t.add(&me.stats);
        match m.outcome_for(puuid) {
            Some((Outcome::Win, ..)) => t.wins += 1,
            Some((Outcome::Loss, ..)) => t.losses += 1,
            _ => {}
        }
    }
    let mut v: Vec<AgentStats> = map
        .into_iter()
        .map(|(agent_id, totals)| AgentStats { agent_id, totals })
        .collect();
    v.sort_by(|a, b| {
        b.totals
            .matches
            .cmp(&a.totals.matches)
            .then(b.totals.kd().total_cmp(&a.totals.kd()))
    });
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Days7,
    Days30,
    Days90,
    All,
}

impl Range {
    pub const ALL: [Range; 4] = [Range::Days7, Range::Days30, Range::Days90, Range::All];

    pub fn label(self) -> &'static str {
        match self {
            Range::Days7 => "7D",
            Range::Days30 => "30D",
            Range::Days90 => "90D",
            Range::All => "All",
        }
    }

    fn seconds(self) -> Option<i64> {
        match self {
            Range::Days7 => Some(7 * 86_400),
            Range::Days30 => Some(30 * 86_400),
            Range::Days90 => Some(90 * 86_400),
            Range::All => None,
        }
    }
}

/// `(start unix seconds, ACS)` per match within `range` of `now`, oldest first.
pub fn acs_series(puuid: &str, matches: &[&MatchDetail], range: Range, now: i64) -> Vec<(i64, f32)> {
    let mut v: Vec<(i64, f32)> = matches
        .iter()
        .filter(|m| range.seconds().is_none_or(|s| m.info.started_at_unix >= now - s))
        .filter_map(|m| m.player(puuid).map(|p| (m.info.started_at_unix, p.stats.acs())))
        .collect();
    v.sort_by_key(|(t, _)| *t);
    v
}

/// Points where the rank changed (first time each new tier was reached),
/// plus the oldest point, oldest first; at most `max` (the most recent).
pub fn rank_milestones(history: &[RrPoint], max: usize) -> Vec<RrPoint> {
    let mut out: Vec<RrPoint> = Vec::new();
    for p in history {
        if out.last().is_none_or(|l| l.tier != p.tier) {
            out.push(p.clone());
        }
    }
    if out.len() > max {
        out = out.split_off(out.len() - max);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityKind {
    RankUp,
    RankDown,
    Match,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    pub kind: ActivityKind,
    pub tier: u32,
    pub map_id: String,
    pub rr_earned: i32,
    pub at_unix: i64,
}

/// Recent ranked events, newest first: promotions, demotions, games played.
pub fn activity(history: &[RrPoint], max: usize) -> Vec<Activity> {
    history
        .iter()
        .rev()
        .map(|p| {
            let kind = if p.tier_before > 0 && p.tier > p.tier_before {
                ActivityKind::RankUp
            } else if p.tier_before > 0 && p.tier < p.tier_before {
                ActivityKind::RankDown
            } else {
                ActivityKind::Match
            };
            Activity {
                kind,
                tier: p.tier,
                map_id: p.map_id.clone(),
                rr_earned: p.earned,
                at_unix: p.at_unix,
            }
        })
        .take(max)
        .collect()
}

/// Loaded details for a list of match ids, keeping their order.
pub fn loaded<'a>(
    ids: impl Iterator<Item = &'a String>,
    cache: &'a HashMap<String, Arc<MatchDetail>>,
) -> Vec<&'a MatchDetail> {
    ids.filter_map(|id| cache.get(id).map(|m| m.as_ref())).collect()
}

/// "2h ago", "3d ago", "just now".
pub fn ago(unix: i64, now: i64) -> String {
    let d = (now - unix).max(0);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86_399 => format!("{}h ago", d / 3600),
        86_400..=2_591_999 => format!("{}d ago", d / 86_400),
        _ => format!("{}mo ago", d / 2_592_000),
    }
}

/// Performance score: how well someone played relative to the other players
/// in the same match, on a 0–1000 scale where 500 is the lobby average.
///
/// Each stat is turned into a z-score against the lobby (capped at ±3) and
/// the weighted sum is mapped to `500 + 160 × z`: ACS 30%, KAST 20%, ADR 15%,
/// K/D 15%, first kills minus first deaths per round 10%, headshot % 10%.
pub const PERFORMANCE_WEIGHTS: [(&str, f32); 6] = [
    ("ACS", 0.30),
    ("KAST", 0.20),
    ("ADR", 0.15),
    ("K/D", 0.15),
    ("Entry (first kills − first deaths per round)", 0.10),
    ("Headshot %", 0.10),
];

/// Performance score for every player in the match, in `players` order.
/// Empty when fewer than two players have rounds to compare.
pub fn performance_scores(detail: &MatchDetail) -> Vec<(String, u32)> {
    let players: Vec<_> = detail.players.iter().filter(|p| p.stats.rounds_played > 0).collect();
    if players.len() < 2 {
        return Vec::new();
    }
    let metrics: Vec<[f32; 6]> = players
        .iter()
        .map(|p| {
            let s = &p.stats;
            let rounds = s.rounds_played as f32;
            [
                s.acs(),
                s.kast_pct(),
                s.adr(),
                s.kd(),
                (s.first_kills as f32 - s.first_deaths as f32) / rounds,
                s.hs_pct(),
            ]
        })
        .collect();
    let n = metrics.len() as f32;
    let mut mean = [0.0f32; 6];
    let mut sd = [0.0f32; 6];
    for i in 0..6 {
        mean[i] = metrics.iter().map(|m| m[i]).sum::<f32>() / n;
        let var = metrics.iter().map(|m| (m[i] - mean[i]).powi(2)).sum::<f32>() / n;
        sd[i] = var.sqrt();
    }
    players
        .iter()
        .zip(&metrics)
        .map(|(p, m)| {
            let z: f32 = (0..6)
                .map(|i| {
                    let zi = if sd[i] > 1e-6 { (m[i] - mean[i]) / sd[i] } else { 0.0 };
                    PERFORMANCE_WEIGHTS[i].1 * zi.clamp(-3.0, 3.0)
                })
                .sum();
            let score = (500.0 + 160.0 * z).round().clamp(0.0, 1000.0) as u32;
            (p.puuid.clone(), score)
        })
        .collect()
}

/// Performance score of one player in a match.
pub fn performance(detail: &MatchDetail, puuid: &str) -> Option<u32> {
    performance_scores(detail)
        .into_iter()
        .find(|(p, _)| p == puuid)
        .map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::riot::models::{MatchInfo, PlayerMatch, PlayerStats, TeamResult};

    fn m(me_kills: u32, won: bool, agent: &str, t: i64) -> MatchDetail {
        let stats = |k: u32| PlayerStats {
            kills: k,
            deaths: 10,
            score: 200 * 20,
            rounds_played: 20,
            damage_made: 3000,
            headshots: 5,
            bodyshots: 15,
            ..Default::default()
        };
        MatchDetail {
            info: MatchInfo {
                started_at_unix: t,
                ..Default::default()
            },
            players: vec![
                PlayerMatch {
                    puuid: "me".into(),
                    team_id: "A".into(),
                    character_id: agent.into(),
                    stats: stats(me_kills),
                    ..Default::default()
                },
                PlayerMatch {
                    puuid: "x".into(),
                    team_id: "B".into(),
                    stats: stats(10),
                    ..Default::default()
                },
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
    fn overview_and_agents() {
        let ms = [
            m(20, true, "jett", 100),
            m(10, false, "jett", 200),
            m(15, true, "sova", 300),
        ];
        let refs: Vec<&MatchDetail> = ms.iter().collect();
        let o = overview("me", &refs);
        assert_eq!((o.me.matches, o.me.wins, o.me.losses), (3, 2, 1));
        assert_eq!(o.me.kd(), 1.5);
        assert_eq!(o.lobby.kd(), 1.0);
        assert_eq!(o.me.acs(), 200.0);
        assert_eq!(o.me.hs_pct(), 25.0);
        assert!((relative_delta(o.me.kd(), o.lobby.kd()).unwrap() - 50.0).abs() < 1e-3);
        let a = agents("me", &refs);
        assert_eq!(a[0].agent_id, "jett");
        assert_eq!(a[0].totals.matches, 2);
        assert_eq!(a[0].totals.win_rate(), Some(50.0));
        assert_eq!(a[1].agent_id, "sova");
    }

    #[test]
    fn series_respects_range() {
        let ms = [
            m(20, true, "a", 1_000),
            m(20, true, "a", 1_000_000),
            m(20, true, "a", 1_500_000),
        ];
        let refs: Vec<&MatchDetail> = ms.iter().collect();
        let now = 1_600_000;
        assert_eq!(acs_series("me", &refs, Range::All, now).len(), 3);
        assert_eq!(acs_series("me", &refs, Range::Days7, now).len(), 2);
        assert_eq!(
            acs_series("me", &refs, Range::Days7, now)[0].0,
            1_000_000,
            "oldest first"
        );
    }

    fn rr(t: i64, before: u32, tier: u32, earned: i32) -> RrPoint {
        RrPoint {
            at_unix: t,
            tier,
            tier_before: before,
            rr: 50,
            earned,
            map_id: "m".into(),
            match_id: String::new(),
        }
    }

    #[test]
    fn milestones_and_activity() {
        let h = [
            rr(1, 9, 9, 10),
            rr(2, 9, 10, 20),
            rr(3, 10, 10, 15),
            rr(4, 10, 9, -20),
            rr(5, 9, 10, 25),
            rr(6, 10, 10, 5),
        ];
        let ms = rank_milestones(&h, 10);
        assert_eq!(
            ms.iter().map(|p| (p.at_unix, p.tier)).collect::<Vec<_>>(),
            vec![(1, 9), (2, 10), (4, 9), (5, 10)]
        );
        assert_eq!(rank_milestones(&h, 2).len(), 2);
        let a = activity(&h, 3);
        assert_eq!(
            a.iter().map(|x| x.kind.clone()).collect::<Vec<_>>(),
            vec![ActivityKind::Match, ActivityKind::RankUp, ActivityKind::RankDown]
        );
    }

    #[test]
    fn relative_times() {
        assert_eq!(ago(100, 110), "just now");
        assert_eq!(ago(0, 7200), "2h ago");
        assert_eq!(ago(0, 3 * 86_400), "3d ago");
    }
}
