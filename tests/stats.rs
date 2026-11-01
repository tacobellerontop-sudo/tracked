//! Stat derivations against a tiny hand-computed 2v2, 3-round match.

use serde_json::json;
use tracked::riot::match_stats::normalize;
use tracked::riot::models::{Outcome, RawMatch};

fn kill(t: u64, killer: &str, victim: &str, assists: &[&str]) -> serde_json::Value {
    json!({ "roundTime": t, "killer": killer, "victim": victim, "assistants": assists })
}

fn dmg(to: &str, d: u32, hs: u32, bs: u32, ls: u32) -> serde_json::Value {
    json!({ "receiver": to, "damage": d, "headshots": hs, "bodyshots": bs, "legshots": ls })
}

fn fixture() -> RawMatch {
    let player = |id: &str, team: &str, k: u32, d: u32, a: u32, score: u32| {
        json!({ "subject": id, "gameName": id, "tagLine": "T", "teamId": team, "characterId": "AGENT",
                "competitiveTier": 12, "accountLevel": 50,
                "stats": { "score": score, "roundsPlayed": 3, "kills": k, "deaths": d, "assists": a } })
    };
    serde_json::from_value(json!({
        "matchInfo": { "matchId": "m1", "mapId": "/Game/Maps/Ascent/Ascent", "queueID": "competitive",
                       "gameLengthMillis": 600000, "gameStartMillis": 1700000000000i64,
                       "isRanked": true, "isCompleted": true, "seasonId": "s1" },
        "players": [
            player("A1", "Red", 1, 3, 0, 300), player("A2", "Red", 3, 1, 1, 600),
            player("B1", "Blue", 2, 2, 0, 400), player("B2", "Blue", 2, 2, 0, 450)
        ],
        "teams": [
            { "teamId": "Red", "won": true, "roundsWon": 2, "roundsPlayed": 3 },
            { "teamId": "Blue", "won": false, "roundsWon": 1, "roundsPlayed": 3 }
        ],
        "roundResults": [
            { "roundNum": 0, "winningTeam": "Red", "winningTeamRole": "Attacker", "bombPlanter": "A1",
              "playerStats": [
                { "subject": "A1", "kills": [kill(1000, "A1", "B1", &["A2"])], "damage": [dmg("B1", 150, 1, 1, 0)] },
                { "subject": "B2", "kills": [kill(3000, "B2", "A1", &[])], "damage": [dmg("A1", 150, 0, 3, 0)] },
                { "subject": "A2", "kills": [kill(4000, "A2", "B2", &[])], "damage": [dmg("B2", 150, 2, 0, 1)] }
              ] },
            { "roundNum": 1, "winningTeam": "Blue", "winningTeamRole": "Defender", "bombDefuser": "B1",
              "playerStats": [
                { "subject": "B1", "kills": [kill(2000, "B1", "A1", &[]), kill(2500, "B1", "A2", &[])],
                  "damage": [dmg("A1", 140, 1, 0, 0), dmg("A2", 140, 1, 0, 0)] }
              ] },
            { "roundNum": 2, "winningTeam": "Red", "winningTeamRole": "Defender",
              "playerStats": [
                { "subject": "B2", "kills": [kill(1000, "B2", "A1", &[])], "damage": [dmg("A1", 150, 0, 3, 0)] },
                { "subject": "A2", "kills": [kill(2000, "A2", "B1", &[]), kill(3000, "A2", "B2", &[])],
                  "damage": [dmg("B1", 150, 1, 1, 0), dmg("B2", 150, 1, 1, 0)] }
              ] }
        ]
    }))
    .unwrap()
}

#[test]
fn match_info_is_normalized() {
    let m = normalize(&fixture());
    assert_eq!(m.info.queue_id, "competitive");
    assert_eq!(m.info.started_at_unix, 1_700_000_000);
    assert_eq!(m.info.game_length_ms, 600_000);
    assert_eq!(m.rounds.len(), 3);
    assert_eq!(m.outcome_for("A1"), Some((Outcome::Win, 2, 1)));
    assert_eq!(m.outcome_for("B1"), Some((Outcome::Loss, 1, 2)));
    assert_eq!(
        m.rounds[0]
            .player_stats
            .iter()
            .find(|p| p.puuid == "A1")
            .unwrap()
            .damage,
        150
    );
}

#[test]
fn a1_stats() {
    let m = normalize(&fixture());
    let s = &m.player("A1").unwrap().stats;
    assert_eq!((s.kills, s.deaths, s.assists), (1, 3, 0));
    assert_eq!(s.damage_made, 150);
    assert_eq!(s.damage_taken, 440);
    assert_eq!(s.kast_rounds, 2, "kill in r0, traded in r2");
    assert_eq!(s.traded_deaths, 2);
    assert_eq!((s.first_kills, s.first_deaths), (1, 2));
    assert_eq!(s.plants, 1);
    assert_eq!((s.attack_rounds_won, s.defence_rounds_won), (1, 1));
    assert!((s.kd() - 1.0 / 3.0).abs() < 1e-5);
}

#[test]
fn a2_stats_and_derivations() {
    let m = normalize(&fixture());
    let s = &m.player("A2").unwrap().stats;
    assert_eq!(s.damage_made, 450);
    assert_eq!(s.damage_taken, 140);
    assert_eq!((s.headshots, s.bodyshots, s.legshots), (4, 2, 1));
    assert_eq!(s.kast_rounds, 2);
    assert_eq!(s.clutches_won, 1, "last alive in r2 and won");
    assert_eq!(s.kd(), 3.0);
    assert_eq!(s.adr(), 150.0);
    assert_eq!(s.acs(), 200.0);
    assert!((s.hs_pct() - 400.0 / 7.0).abs() < 1e-3);
    assert!((s.kast_pct() - 200.0 / 3.0).abs() < 1e-3);
}

#[test]
fn blue_side_stats() {
    let m = normalize(&fixture());
    let b1 = &m.player("B1").unwrap().stats;
    assert_eq!(b1.damage_made, 280);
    assert_eq!(b1.kast_rounds, 2, "traded in r0 (B2 kills A1 2s later), kill in r1");
    assert_eq!(b1.traded_deaths, 1);
    assert_eq!(b1.defuses, 1);
    assert_eq!(b1.defence_rounds_won, 1);
    assert_eq!(b1.clutches_won, 0);
    let b2 = &m.player("B2").unwrap().stats;
    assert_eq!(b2.kast_rounds, 3, "kill, survive, kill");
    assert_eq!(b2.first_kills, 1);
    assert_eq!(b2.traded_deaths, 0);
}

#[test]
fn multikills_and_aces() {
    let mut players = vec![json!({ "subject": "A", "teamId": "Red" })];
    let mut kills = Vec::new();
    for i in 0..5 {
        players.push(json!({ "subject": format!("V{i}"), "teamId": "Blue" }));
        kills.push(kill(1000 + i * 100, "A", &format!("V{i}"), &[]));
    }
    let three: Vec<_> = kills[..3].to_vec();
    let four: Vec<_> = kills[..4].to_vec();
    let raw: RawMatch = serde_json::from_value(json!({
        "players": players,
        "roundResults": [
            { "winningTeam": "Red", "playerStats": [{ "subject": "A", "kills": kills }] },
            { "winningTeam": "Red", "playerStats": [{ "subject": "A", "kills": three }] },
            { "winningTeam": "Red", "playerStats": [{ "subject": "A", "kills": four }] }
        ]
    }))
    .unwrap();
    let s = normalize(&raw).player("A").unwrap().stats.clone();
    assert_eq!((s.multi_kill_rounds3, s.multi_kill_rounds4, s.aces), (1, 1, 1));
    assert_eq!(s.rounds_played, 3, "falls back to round count");
}

#[test]
fn empty_match_does_not_panic() {
    let m = normalize(&RawMatch::default());
    assert!(m.players.is_empty());
    let s = tracked::riot::models::PlayerStats::default();
    assert_eq!(
        (s.kd(), s.adr(), s.hs_pct(), s.kast_pct(), s.acs()),
        (0.0, 0.0, 0.0, 0.0, 0.0)
    );
}

#[test]
fn performance_score_centres_on_the_lobby() {
    for seed in 1..6 {
        let m = tracked::worker::demo::synthetic_match(seed);
        let scores = tracked::stats::performance_scores(&m);
        assert_eq!(scores.len(), m.players.len());
        let mean = scores.iter().map(|s| s.1 as f32).sum::<f32>() / scores.len() as f32;
        assert!((mean - 500.0).abs() < 5.0, "seed {seed}: mean {mean}");
        assert!(scores.iter().all(|s| s.1 <= 1000));
        // The top player by ACS rates above the lobby average, the bottom one below.
        let by_acs = |best: bool| {
            m.players
                .iter()
                .max_by(|a, b| {
                    let (x, y) = (a.stats.acs(), b.stats.acs());
                    if best {
                        x.total_cmp(&y)
                    } else {
                        y.total_cmp(&x)
                    }
                })
                .unwrap()
                .puuid
                .clone()
        };
        let score_of = |p: &str| scores.iter().find(|s| s.0 == p).unwrap().1;
        assert!(score_of(&by_acs(true)) > 500, "seed {seed}");
        assert!(score_of(&by_acs(false)) < 500, "seed {seed}");
    }
    // Weights add up to 100%.
    let total: f32 = tracked::stats::PERFORMANCE_WEIGHTS.iter().map(|w| w.1).sum();
    assert!((total - 1.0).abs() < 1e-6);
}

const VANDAL: &str = "9c82e19d-4575-0200-1a81-3eacf00cf872";
const SHERIFF: &str = "e336c6b8-418d-9340-d77f-7a9e4cfe0702";

/// Per-weapon kills are exact; hits on a victim go to the gun that killed
/// them, other hits to the gun carried into the round.
#[test]
fn weapon_stats() {
    let k = |t: u64, killer: &str, victim: &str, ty: &str, item: &str| {
        json!({ "roundTime": t, "killer": killer, "victim": victim, "assistants": [],
                "finishingDamage": { "damageType": ty, "damageItem": item } })
    };
    let eco = |w: &str| json!({ "weapon": w, "loadoutValue": 3900 });
    let raw: RawMatch = serde_json::from_value(json!({
        "matchInfo": { "matchId": "w1", "isCompleted": true },
        "players": [
            { "subject": "A", "teamId": "Red" }, { "subject": "B", "teamId": "Blue" },
            { "subject": "C", "teamId": "Blue" }, { "subject": "D", "teamId": "Blue" }
        ],
        "teams": [],
        "roundResults": [
            { "roundNum": 0, "winningTeam": "Red", "playerStats": [
                { "subject": "A", "economy": eco(VANDAL),
                  "kills": [k(1000, "A", "B", "Weapon", &VANDAL.to_uppercase()),
                            k(2000, "A", "C", "Weapon", SHERIFF),
                            k(3000, "A", "D", "Ability", "Ultimate")],
                  // B: headshots only (headshot kill). C: killed with the Sheriff.
                  // D: killed by an ability, so the hits count for the carried Vandal.
                  "damage": [dmg("B", 150, 1, 0, 0), dmg("C", 100, 1, 1, 0), dmg("D", 40, 0, 1, 0)] }
            ] },
            { "roundNum": 1, "winningTeam": "Red", "playerStats": [
                // No economy data: hits go to the only gun A killed with this round.
                { "subject": "A", "kills": [k(1000, "A", "B", "Melee", ""),
                                            k(1500, "A", "C", "Weapon", SHERIFF)],
                  "damage": [dmg("B", 20, 0, 0, 1), dmg("C", 160, 1, 0, 0)] }
            ] }
        ]
    }))
    .unwrap();
    let m = normalize(&raw);
    let a = &m.player("A").unwrap().stats.weapons;
    let get = |id: &str| a.iter().find(|w| w.weapon_id == id).unwrap();

    let sheriff = get(SHERIFF);
    assert_eq!((sheriff.kills, sheriff.headshot_kills, sheriff.rounds), (2, 1, 0));
    assert_eq!((sheriff.damage, sheriff.headshots, sheriff.bodyshots), (260, 2, 1));

    let vandal = get(VANDAL);
    assert_eq!((vandal.kills, vandal.headshot_kills, vandal.rounds), (1, 1, 1));
    assert_eq!(
        (vandal.damage, vandal.headshots, vandal.bodyshots, vandal.legshots),
        (190, 1, 1, 0)
    );

    let melee = get(tracked::content::weapons::MELEE);
    assert_eq!((melee.kills, melee.damage), (1, 20));
    let abilities = get(tracked::content::weapons::ABILITIES);
    assert_eq!((abilities.kills, abilities.damage), (1, 0));

    // Most kills first; ties by damage.
    assert_eq!(a[0].weapon_id, SHERIFF);
    assert_eq!(a[1].weapon_id, VANDAL);

    let totals = tracked::stats::weapons("A", &[&m, &m]);
    assert_eq!(totals[0].stat.kills, 4);
    assert_eq!(totals[0].matches, 2);
    assert!((totals[0].kill_share - 40.0).abs() < 0.01);
    assert_eq!(totals[1].damage_per_round(), Some(190.0));
}
