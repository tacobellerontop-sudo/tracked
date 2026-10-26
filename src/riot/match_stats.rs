//! Turn a raw Riot match into [`MatchDetail`], computing every derived stat
//! ourselves from the round-by-round data.

use std::collections::{HashMap, HashSet};

use super::models::*;

/// A death counts as traded if a teammate kills the killer within this window.
pub const TRADE_WINDOW_MS: u64 = 5_000;

impl PlayerStats {
    pub fn kd(&self) -> f32 {
        self.kills as f32 / self.deaths.max(1) as f32
    }

    /// Average damage per round.
    pub fn adr(&self) -> f32 {
        ratio(self.damage_made, self.rounds_played)
    }

    /// Average combat score.
    pub fn acs(&self) -> f32 {
        ratio(self.score, self.rounds_played)
    }

    /// Headshot percentage over all hits (0-100).
    pub fn hs_pct(&self) -> f32 {
        100.0 * ratio(self.headshots, self.headshots + self.bodyshots + self.legshots)
    }

    /// Percentage of rounds with a Kill, Assist, Survival or Trade (0-100).
    pub fn kast_pct(&self) -> f32 {
        100.0 * ratio(self.kast_rounds, self.rounds_played)
    }
}

fn ratio(a: u32, b: u32) -> f32 {
    if b == 0 {
        0.0
    } else {
        a as f32 / b as f32
    }
}

/// Normalize a raw match. Never panics on partial data.
pub fn normalize(raw: &RawMatch) -> MatchDetail {
    let team_of: HashMap<&str, &str> = raw
        .players
        .iter()
        .map(|p| (p.subject.as_str(), p.team_id.as_str()))
        .collect();
    let mut stats: HashMap<String, PlayerStats> = HashMap::new();

    for p in &raw.players {
        let s = stats.entry(p.subject.clone()).or_default();
        if let Some(ps) = &p.stats {
            s.score = ps.score;
            s.kills = ps.kills;
            s.deaths = ps.deaths;
            s.assists = ps.assists;
            s.rounds_played = ps.rounds_played;
        }
    }

    let mut rounds = Vec::with_capacity(raw.round_results.len());
    for round in &raw.round_results {
        rounds.push(apply_round(round, &team_of, &mut stats));
    }

    // Fall back to the number of rounds if the per-player count is missing.
    for s in stats.values_mut() {
        if s.rounds_played == 0 {
            s.rounds_played = raw.round_results.len() as u32;
        }
    }

    let players = raw
        .players
        .iter()
        .map(|p| PlayerMatch {
            puuid: p.subject.clone(),
            game_name: p.game_name.clone(),
            tag_line: p.tag_line.clone(),
            team_id: p.team_id.clone(),
            party_id: p.party_id.clone(),
            character_id: p.character_id.to_ascii_lowercase(),
            competitive_tier: p.competitive_tier,
            account_level: p.account_level,
            stats: stats.get(&p.subject).cloned().unwrap_or_default(),
        })
        .collect();

    let i = &raw.match_info;
    MatchDetail {
        info: MatchInfo {
            match_id: i.match_id.clone(),
            map_id: i.map_id.clone(),
            queue_id: i.queue_id.clone(),
            game_mode: i.game_mode.clone(),
            game_length_ms: i.game_length_millis,
            started_at_unix: i.game_start_millis / 1000,
            is_ranked: i.is_ranked,
            is_completed: i.is_completed,
            season_id: i.season_id.clone(),
        },
        players,
        teams: raw
            .teams
            .iter()
            .map(|t| TeamResult {
                team_id: t.team_id.clone(),
                won: t.won,
                rounds_won: t.rounds_won,
                rounds_played: t.rounds_played,
            })
            .collect(),
        rounds,
    }
}

fn apply_round(
    round: &RawRound,
    team_of: &HashMap<&str, &str>,
    stats: &mut HashMap<String, PlayerStats>,
) -> RoundResult {
    let mut kills: Vec<&RawKill> = round.player_stats.iter().flat_map(|ps| ps.kills.iter()).collect();
    kills.sort_by_key(|k| k.round_time);

    let mut kills_by: HashMap<&str, u32> = HashMap::new();
    let mut assisted: HashSet<&str> = HashSet::new();
    let mut died: HashSet<&str> = HashSet::new();
    let mut traded: HashSet<&str> = HashSet::new();

    for (idx, k) in kills.iter().enumerate() {
        *kills_by.entry(k.killer.as_str()).or_default() += 1;
        died.insert(k.victim.as_str());
        for a in &k.assistants {
            assisted.insert(a.as_str());
        }
        // Traded: someone on the victim's team kills this killer shortly after.
        let victim_team = team_of.get(k.victim.as_str());
        let avenged = kills[idx + 1..]
            .iter()
            .take_while(|later| later.round_time <= k.round_time + TRADE_WINDOW_MS)
            .any(|later| {
                later.victim == k.killer && victim_team.is_some() && team_of.get(later.killer.as_str()) == victim_team
            });
        if avenged {
            traded.insert(k.victim.as_str());
        }
    }

    if let Some(first) = kills.first() {
        stats.entry(first.killer.clone()).or_default().first_kills += 1;
        stats.entry(first.victim.clone()).or_default().first_deaths += 1;
    }

    // Clutch: the last player alive on a team (with enemies still up) whose team wins.
    let clutcher = find_clutcher(&kills, team_of);

    let mut damage_by: HashMap<&str, u32> = HashMap::new();
    for ps in &round.player_stats {
        for d in &ps.damage {
            let dealer = stats.entry(ps.subject.clone()).or_default();
            dealer.damage_made += d.damage;
            dealer.headshots += d.headshots;
            dealer.bodyshots += d.bodyshots;
            dealer.legshots += d.legshots;
            *damage_by.entry(ps.subject.as_str()).or_default() += d.damage;
            stats.entry(d.receiver.clone()).or_default().damage_taken += d.damage;
        }
    }

    for (&puuid, &team) in team_of {
        let s = stats.entry(puuid.to_string()).or_default();
        let k = kills_by.get(puuid).copied().unwrap_or(0);
        if k > 0 || assisted.contains(puuid) || !died.contains(puuid) || traded.contains(puuid) {
            s.kast_rounds += 1;
        }
        if traded.contains(puuid) {
            s.traded_deaths += 1;
        }
        match k {
            3 => s.multi_kill_rounds3 += 1,
            4 => s.multi_kill_rounds4 += 1,
            k if k >= 5 => s.aces += 1,
            _ => {}
        }
        if team == round.winning_team {
            match round.winning_team_role.as_deref() {
                Some("Attacker") => s.attack_rounds_won += 1,
                Some("Defender") => s.defence_rounds_won += 1,
                _ => {}
            }
        }
    }

    if let Some((who, team)) = clutcher {
        if team == round.winning_team {
            stats.entry(who).or_default().clutches_won += 1;
        }
    }
    if let Some(p) = round.bomb_planter.as_deref().filter(|p| !p.is_empty()) {
        stats.entry(p.to_string()).or_default().plants += 1;
    }
    if let Some(d) = round.bomb_defuser.as_deref().filter(|d| !d.is_empty()) {
        stats.entry(d.to_string()).or_default().defuses += 1;
    }

    RoundResult {
        round_num: round.round_num,
        winning_team: round.winning_team.clone(),
        winning_team_role: round.winning_team_role.clone(),
        round_ceremony: round.round_ceremony.clone(),
        round_result: round.round_result.clone(),
        player_stats: round
            .player_stats
            .iter()
            .map(|ps| RoundPlayerStat {
                puuid: ps.subject.clone(),
                kills: kills_by.get(ps.subject.as_str()).copied().unwrap_or(0),
                damage: damage_by.get(ps.subject.as_str()).copied().unwrap_or(0),
                score: ps.score,
            })
            .collect(),
    }
}

/// Returns `(puuid, team)` of the first player left alone against at least
/// one living enemy, if any.
fn find_clutcher(kills: &[&RawKill], team_of: &HashMap<&str, &str>) -> Option<(String, String)> {
    let mut alive: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (&p, &t) in team_of {
        alive.entry(t).or_default().insert(p);
    }
    if alive.len() != 2 {
        return None;
    }
    for k in kills {
        let Some(&team) = team_of.get(k.victim.as_str()) else {
            continue;
        };
        if let Some(set) = alive.get_mut(team) {
            set.remove(k.victim.as_str());
            if set.len() == 1 {
                let enemies_alive = alive.iter().any(|(t, s)| *t != team && !s.is_empty());
                if enemies_alive {
                    let last = *alive[team].iter().next().unwrap();
                    return Some((last.to_string(), team.to_string()));
                }
            }
        }
    }
    None
}
