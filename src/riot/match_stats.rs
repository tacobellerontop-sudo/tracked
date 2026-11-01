//! Turn a raw Riot match into [`MatchDetail`], computing every derived stat
//! ourselves from the round-by-round data.

use std::collections::{HashMap, HashSet};

use super::models::*;
use crate::content::weapons;

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
    let mut weapons: HashMap<String, HashMap<String, WeaponStat>> = HashMap::new();
    for round in &raw.round_results {
        rounds.push(apply_round(round, &team_of, &mut stats));
        apply_round_weapons(round, &mut weapons);
    }
    for (puuid, by_weapon) in weapons {
        let mut list: Vec<WeaponStat> = by_weapon.into_values().collect();
        sort_weapons(&mut list);
        stats.entry(puuid).or_default().weapons = list;
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
        schema: MatchDetail::SCHEMA,
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

/// Most kills first, then most damage; ties broken by id so the order is stable.
pub fn sort_weapons(list: &mut [WeaponStat]) {
    list.sort_by(|a, b| {
        b.kills
            .cmp(&a.kills)
            .then(b.damage.cmp(&a.damage))
            .then(a.weapon_id.cmp(&b.weapon_id))
    });
}

/// The weapon bucket a kill belongs to, or `None` for spike / fall damage.
fn kill_weapon(k: &RawKill) -> Option<String> {
    let f = &k.finishing_damage;
    match f.damage_type.as_str() {
        "Weapon" if !f.damage_item.is_empty() => Some(f.damage_item.to_ascii_lowercase()),
        "Melee" => Some(weapons::MELEE.to_string()),
        "Ability" => Some(weapons::ABILITIES.to_string()),
        _ => None,
    }
}

/// Add one round's per-weapon kills, hits and damage (see [`WeaponStat`]).
fn apply_round_weapons(round: &RawRound, out: &mut HashMap<String, HashMap<String, WeaponStat>>) {
    // Hits on each victim this round, by attacker.
    let mut hits: HashMap<(&str, &str), &RawDamage> = HashMap::new();
    for ps in &round.player_stats {
        for d in &ps.damage {
            hits.insert((ps.subject.as_str(), d.receiver.as_str()), d);
        }
    }
    // Which weapon finished each (killer, victim) pair.
    let mut killed_with: HashMap<(&str, &str), String> = HashMap::new();
    for ps in &round.player_stats {
        for k in &ps.kills {
            let Some(w) = kill_weapon(k) else { continue };
            let entry = out
                .entry(k.killer.clone())
                .or_default()
                .entry(w.clone())
                .or_insert_with(|| WeaponStat {
                    weapon_id: w.clone(),
                    ..Default::default()
                });
            entry.kills += 1;
            let headshot_only = hits
                .get(&(k.killer.as_str(), k.victim.as_str()))
                .is_some_and(|d| d.headshots > 0 && d.bodyshots == 0 && d.legshots == 0);
            if headshot_only && weapons::is_gun(&w) {
                entry.headshot_kills += 1;
            }
            killed_with.insert((k.killer.as_str(), k.victim.as_str()), w);
        }
    }
    for ps in &round.player_stats {
        let carried = ps
            .economy
            .as_ref()
            .map(|e| e.weapon.to_ascii_lowercase())
            .filter(|w| !w.is_empty());
        if let Some(w) = &carried {
            let entry = out
                .entry(ps.subject.clone())
                .or_default()
                .entry(w.clone())
                .or_insert_with(|| WeaponStat {
                    weapon_id: w.clone(),
                    ..Default::default()
                });
            entry.rounds += 1;
        }
        // Without economy data, fall back to the only gun this player killed with this round.
        let fallback = || {
            let mut guns = killed_with
                .iter()
                .filter(|((killer, _), w)| *killer == ps.subject && weapons::is_gun(w))
                .map(|(_, w)| w.as_str());
            let first = guns.next()?;
            guns.all(|w| w == first).then(|| first.to_string())
        };
        for d in &ps.damage {
            // Ability kills say nothing about which gun landed the hits.
            let weapon = killed_with
                .get(&(ps.subject.as_str(), d.receiver.as_str()))
                .filter(|w| *w != weapons::ABILITIES)
                .cloned()
                .or_else(|| carried.clone())
                .or_else(fallback);
            let Some(w) = weapon else { continue };
            let entry = out
                .entry(ps.subject.clone())
                .or_default()
                .entry(w.clone())
                .or_insert_with(|| WeaponStat {
                    weapon_id: w.clone(),
                    ..Default::default()
                });
            entry.damage += d.damage;
            entry.headshots += d.headshots;
            entry.bodyshots += d.bodyshots;
            entry.legshots += d.legshots;
        }
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
