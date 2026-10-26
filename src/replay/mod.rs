//! 2D top-down match "replay": a reconstruction from the match-details API.
//!
//! Riot's match data only records where everyone stood (and which way they
//! faced) at the instant of each kill, plant and defuse. A replay is therefore
//! a sequence of freeze-frame snapshots per round, not continuous movement and
//! not VALORANT's own replay files (which third parties can't access).

pub mod calibrate;
pub mod fetch;
pub mod timeline;

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use fetch::{FinishingDamage, MatchDetailsRaw, PlayerLocRaw, RoundRaw};

/// A VALORANT world position.
#[derive(Debug, Default, Clone, Copy, PartialEq, Deserialize)]
#[serde(default)]
pub struct Loc {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerMeta {
    pub puuid: String,
    pub name: String,
    pub tag: String,
    pub team: String,
    pub agent: String,
    pub tier: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamResult {
    pub team: String,
    pub won: bool,
    pub rounds_won: u32,
}

/// Whether a replay can be shown for this match.
#[derive(Debug, Clone, PartialEq)]
pub enum Availability {
    Available,
    /// Mode without meaningful rounds/sides (Deathmatch, Escalation, …).
    UnsupportedMode(String),
    /// Standard mode, but the data carries no positions at all.
    NoPositions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Kill,
    Plant,
    Defuse,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedPlayer {
    pub puuid: String,
    pub team: String,
    pub agent: String,
    pub alive: bool,
    pub pos: Loc,
    /// World-space yaw; `None` for dead players (only their death spot is known).
    pub facing: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KillLink {
    pub killer: String,
    pub victim: String,
    pub assistants: Vec<String>,
    pub damage: FinishingDamage,
    /// Every hit the killer landed on the victim this round was a headshot,
    /// so the finishing hit was too. (The API has no per-kill headshot flag.)
    pub headshot: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub kind: FrameKind,
    pub pos: Loc,
    pub site: Option<String>,
    pub by: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub t_ms: u32,
    pub kind: FrameKind,
    pub positions: Vec<PlacedPlayer>,
    pub kill: Option<KillLink>,
    pub marker: Option<Marker>,
}

/// One line of a round's kill feed. Every kill is listed, including ones
/// whose event carried no positions (`frame` is then `None`).
#[derive(Debug, Clone, PartialEq)]
pub struct FeedEntry {
    pub t_ms: u32,
    pub kill: KillLink,
    pub frame: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoundReplay {
    pub num: u32,
    pub winning_team: String,
    pub result: String,
    /// Team on attack this round ("Red"/"Blue"), when known.
    pub attackers: Option<String>,
    pub frames: Vec<Frame>,
    pub feed: Vec<FeedEntry>,
    pub plant_site: Option<String>,
    /// Continuous playback of this round (see [`timeline`]).
    pub timeline: timeline::RoundTimeline,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchReplay {
    pub match_id: String,
    pub map_id: String,
    pub queue_id: String,
    pub game_mode: String,
    pub started_at_ms: i64,
    pub players: Vec<PlayerMeta>,
    pub teams: Vec<TeamResult>,
    pub rounds: Vec<RoundReplay>,
    pub availability: Availability,
}

impl MatchReplay {
    pub fn player(&self, puuid: &str) -> Option<&PlayerMeta> {
        self.players.iter().find(|p| p.puuid == puuid)
    }
}

/// Modes where a top-down round replay makes no sense.
pub fn availability_for(queue_id: &str, game_mode: &str) -> Availability {
    let q = queue_id.to_ascii_lowercase();
    let m = game_mode.to_ascii_lowercase();
    let unsupported = [
        ("deathmatch", "Deathmatch"),
        ("hurm", "Team Deathmatch"),
        ("ggteam", "Escalation"),
        ("onefa", "Replication"),
        ("snowball", "Snowball Fight"),
    ];
    for (key, name) in unsupported {
        if q == key || m.contains(key) {
            return Availability::UnsupportedMode(name.to_string());
        }
    }
    if m.contains("teamdeathmatch") {
        return Availability::UnsupportedMode("Team Deathmatch".into());
    }
    Availability::Available
}

/// Turn the raw match into per-round frame sequences.
pub fn from_raw(raw: &MatchDetailsRaw) -> MatchReplay {
    let players: Vec<PlayerMeta> = raw
        .players
        .iter()
        .map(|p| PlayerMeta {
            puuid: p.subject.clone(),
            name: p.game_name.clone(),
            tag: p.tag_line.clone(),
            team: p.team_id.clone(),
            agent: p.character_id.to_ascii_lowercase(),
            tier: p.competitive_tier,
        })
        .collect();
    let team_of: HashMap<&str, &PlayerMeta> = players.iter().map(|p| (p.puuid.as_str(), p)).collect();
    let mut rounds: Vec<RoundReplay> = raw.round_results.iter().map(|r| build_round(r, &team_of)).collect();
    rounds.sort_by_key(|r| r.num);

    let info = &raw.match_info;
    let mut availability = availability_for(&info.queue_id, &info.game_mode);
    if availability == Availability::Available {
        let any_positions = rounds.iter().any(|r| !r.frames.is_empty());
        let any_kills = rounds.iter().any(|r| !r.feed.is_empty());
        if any_kills && !any_positions {
            availability = Availability::NoPositions;
        }
    }
    MatchReplay {
        match_id: info.match_id.clone(),
        map_id: info.map_id.clone(),
        queue_id: info.queue_id.clone(),
        game_mode: info.game_mode.clone(),
        started_at_ms: info.game_start_millis,
        teams: raw
            .teams
            .iter()
            .map(|t| TeamResult {
                team: t.team_id.clone(),
                won: t.won,
                rounds_won: t.rounds_won,
            })
            .collect(),
        players,
        rounds,
        availability,
    }
}

/// Attacking team for round `n` (0-based) by the standard side-swap rules:
/// Red attacks rounds 1–12, Blue 13–24, then sides alternate every round in
/// overtime. A planter's team overrides this when present.
pub fn attackers_by_rule(n: u32) -> &'static str {
    match n {
        0..=11 => "Red",
        12..=23 => "Blue",
        _ if n.is_multiple_of(2) => "Red",
        _ => "Blue",
    }
}

fn placed(locs: &[PlayerLocRaw], team_of: &HashMap<&str, &PlayerMeta>) -> Vec<PlacedPlayer> {
    locs.iter()
        .map(|l| {
            let meta = team_of.get(l.subject.as_str());
            PlacedPlayer {
                puuid: l.subject.clone(),
                team: meta.map(|m| m.team.clone()).unwrap_or_default(),
                agent: meta.map(|m| m.agent.clone()).unwrap_or_default(),
                alive: true,
                pos: l.location,
                facing: Some(l.view_radians),
            }
        })
        .collect()
}

/// Build one round's frames, in time order, with dead players carried
/// forward at the spot they died.
pub fn build_round(r: &RoundRaw, team_of: &HashMap<&str, &PlayerMeta>) -> RoundReplay {
    enum Event<'a> {
        Kill(&'a fetch::KillRaw, bool),
        Plant,
        Defuse,
    }
    // Headshot: every hit from killer → victim this round was a headshot.
    let mut hs_only: HashSet<(&str, &str)> = HashSet::new();
    for ps in &r.player_stats {
        for d in &ps.damage {
            if d.headshots > 0 && d.bodyshots == 0 && d.legshots == 0 {
                hs_only.insert((ps.subject.as_str(), d.receiver.as_str()));
            }
        }
    }
    let mut events: Vec<(u32, Event)> = Vec::new();
    for ps in &r.player_stats {
        for k in &ps.kills {
            let hs =
                k.finishing_damage.damage_type == "Weapon" && hs_only.contains(&(k.killer.as_str(), k.victim.as_str()));
            events.push((k.round_time, Event::Kill(k, hs)));
        }
    }
    if let (Some(t), Some(_)) = (r.plant_round_time, &r.plant_location) {
        if r.bomb_planter.as_deref().is_some_and(|p| !p.is_empty()) {
            events.push((t, Event::Plant));
        }
    }
    if let (Some(t), Some(_)) = (r.defuse_round_time, &r.defuse_location) {
        if r.bomb_defuser.as_deref().is_some_and(|p| !p.is_empty()) {
            events.push((t, Event::Defuse));
        }
    }
    // Stable sort: same-timestamp events keep their API order.
    events.sort_by_key(|(t, _)| *t);

    let mut dead: Vec<PlacedPlayer> = Vec::new();
    let mut frames = Vec::new();
    let mut feed = Vec::new();
    for (t, ev) in events {
        match ev {
            Event::Kill(k, headshot) => {
                let link = KillLink {
                    killer: k.killer.clone(),
                    victim: k.victim.clone(),
                    assistants: k.assistants.clone(),
                    damage: k.finishing_damage.clone(),
                    headshot,
                };
                // Where the victim fell: the event's victimLocation, else their
                // own entry in the snapshot.
                let death_spot = k.victim_location.or_else(|| {
                    k.player_locations
                        .iter()
                        .find(|l| l.subject == k.victim)
                        .map(|l| l.location)
                });
                let frame = if k.player_locations.is_empty() {
                    None
                } else {
                    let mut positions = placed(&k.player_locations, team_of);
                    positions.retain(|p| p.puuid != k.victim && !dead.iter().any(|d| d.puuid == p.puuid));
                    positions.extend(dead.iter().cloned());
                    if let Some(spot) = death_spot {
                        let meta = team_of.get(k.victim.as_str());
                        positions.push(PlacedPlayer {
                            puuid: k.victim.clone(),
                            team: meta.map(|m| m.team.clone()).unwrap_or_default(),
                            agent: meta.map(|m| m.agent.clone()).unwrap_or_default(),
                            alive: false,
                            pos: spot,
                            facing: None,
                        });
                    }
                    frames.push(Frame {
                        t_ms: t,
                        kind: FrameKind::Kill,
                        positions,
                        kill: Some(link.clone()),
                        marker: None,
                    });
                    Some(frames.len() - 1)
                };
                feed.push(FeedEntry {
                    t_ms: t,
                    kill: link,
                    frame,
                });
                if let Some(spot) = death_spot {
                    let meta = team_of.get(k.victim.as_str());
                    dead.push(PlacedPlayer {
                        puuid: k.victim.clone(),
                        team: meta.map(|m| m.team.clone()).unwrap_or_default(),
                        agent: meta.map(|m| m.agent.clone()).unwrap_or_default(),
                        alive: false,
                        pos: spot,
                        facing: None,
                    });
                }
            }
            Event::Plant | Event::Defuse => {
                let (kind, locs, pos, by) = match ev {
                    Event::Plant => (
                        FrameKind::Plant,
                        r.plant_player_locations.as_deref().unwrap_or_default(),
                        r.plant_location.unwrap_or_default(),
                        r.bomb_planter.clone(),
                    ),
                    _ => (
                        FrameKind::Defuse,
                        r.defuse_player_locations.as_deref().unwrap_or_default(),
                        r.defuse_location.unwrap_or_default(),
                        r.bomb_defuser.clone(),
                    ),
                };
                let mut positions = placed(locs, team_of);
                positions.retain(|p| !dead.iter().any(|d| d.puuid == p.puuid));
                positions.extend(dead.iter().cloned());
                frames.push(Frame {
                    t_ms: t,
                    kind,
                    positions,
                    kill: None,
                    marker: Some(Marker {
                        kind,
                        pos,
                        site: r.plant_site.clone().filter(|s| !s.is_empty()),
                        by,
                    }),
                });
            }
        }
    }

    let planter_team = r
        .bomb_planter
        .as_deref()
        .and_then(|p| team_of.get(p))
        .map(|m| m.team.clone());
    let mut round = RoundReplay {
        num: r.round_num,
        winning_team: r.winning_team.clone(),
        result: r.round_result.clone(),
        attackers: planter_team.or_else(|| Some(attackers_by_rule(r.round_num).to_string())),
        frames,
        feed,
        plant_site: r.plant_site.clone().filter(|s| !s.is_empty()),
        timeline: timeline::RoundTimeline::default(),
    };
    round.timeline = timeline::RoundTimeline::build(&round);
    round
}
