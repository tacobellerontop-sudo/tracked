//! 2D replay: parsing, world→minimap transform, frame assembly, mode gating.

use tracked::replay::calibrate::{facing, world_to_norm, MapCal};
use tracked::replay::fetch::MatchDetailsRaw;
use tracked::replay::{availability_for, from_raw, Availability, FrameKind, Loc};

const A1: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const A2: &str = "aaaaaaaa-0000-4000-8000-000000000002";
const B3: &str = "bbbbbbbb-0000-4000-8000-000000000003";
const B4: &str = "bbbbbbbb-0000-4000-8000-000000000004";

fn fixture() -> MatchDetailsRaw {
    serde_json::from_str(include_str!("fixtures/match_details_trimmed.json")).expect("fixture parses")
}

#[test]
fn parses_the_fields_the_replay_uses() {
    let raw = fixture();
    assert_eq!(raw.match_info.map_id, "/Game/Maps/Ascent/Ascent");
    assert_eq!(raw.match_info.queue_id, "competitive");
    assert_eq!(raw.players.len(), 4);
    assert_eq!(raw.players[0].character_id, "ADD6443A-41BD-E414-F6AD-E58D267F4E95");
    let r0 = &raw.round_results[0];
    assert_eq!(r0.bomb_planter.as_deref(), Some(A2));
    assert_eq!(r0.plant_round_time, Some(41000));
    assert_eq!(r0.plant_location, Some(Loc { x: 6600.0, y: -7060.0 }));
    assert_eq!(r0.plant_site.as_deref(), Some("A"));
    assert_eq!(r0.plant_player_locations.as_ref().map(Vec::len), Some(3));
    let k = &r0.player_stats[0].kills[1];
    assert_eq!((k.round_time, k.game_time), (22000, 120200));
    assert_eq!(k.victim, B3);
    assert_eq!(k.assistants, vec![A2.to_string()]);
    assert_eq!(k.victim_location, Some(Loc { x: 3000.0, y: -6000.0 }));
    assert_eq!(k.player_locations.len(), 3);
    assert_eq!(k.player_locations[0].subject, A1);
    assert!((k.player_locations[0].view_radians - 0.5).abs() < 1e-6);
    assert_eq!(k.finishing_damage.damage_type, "Weapon");
    assert_eq!(k.finishing_damage.damage_item, "9C82E19D-4575-0200-1A81-3EACF00CF872");
    // A round with `null` location lists and no kills still parses.
    assert!(raw.round_results[1].plant_player_locations.is_none());
}

#[test]
fn official_api_field_names_are_accepted_too() {
    let k: tracked::replay::fetch::KillRaw = serde_json::from_value(serde_json::json!({
        "timeSinceRoundStartMillis": 1234, "timeSinceGameStartMillis": 99999,
        "killer": "k", "victim": "v",
        "playerLocations": [{ "puuid": "k", "viewRadians": 1.0, "location": { "x": 1, "y": 2 } }]
    }))
    .unwrap();
    assert_eq!((k.round_time, k.game_time), (1234, 99999));
    assert_eq!(k.player_locations[0].subject, "k");
}

#[test]
fn frames_are_time_ordered_and_the_dead_set_advances() {
    let replay = from_raw(&fixture());
    assert_eq!(replay.availability, Availability::Available);
    let r = &replay.rounds[0];
    // Events arrive out of order (62s, 22s from one player, 47s from another,
    // plant at 41s); frames come out sorted. The 47s kill has no positions,
    // so it's in the feed but has no frame.
    let times: Vec<(u32, FrameKind)> = r.frames.iter().map(|f| (f.t_ms, f.kind)).collect();
    assert_eq!(
        times,
        vec![
            (22000, FrameKind::Kill),
            (41000, FrameKind::Plant),
            (62000, FrameKind::Kill)
        ]
    );
    let feed: Vec<(u32, Option<usize>)> = r.feed.iter().map(|e| (e.t_ms, e.frame)).collect();
    assert_eq!(feed, vec![(22000, Some(0)), (47000, None), (62000, Some(2))]);

    let alive = |i: usize, p: &str| r.frames[i].positions.iter().find(|x| x.puuid == p).map(|x| x.alive);
    // Frame 0: B3 dies.
    assert_eq!(alive(0, B3), Some(false));
    assert_eq!(alive(0, A1), Some(true));
    // Plant frame: B3 still dead (carried forward at its death spot).
    assert_eq!(alive(1, B3), Some(false));
    assert_eq!(alive(1, A2), Some(true));
    // Last frame: B3 and A2 (killed at 47s, feed-only) are dead, B4 dies now.
    assert_eq!(alive(2, B3), Some(false));
    assert_eq!(alive(2, A2), Some(false));
    assert_eq!(alive(2, B4), Some(false));
    assert_eq!(alive(2, A1), Some(true));
    // Nobody appears twice in a frame.
    for f in &r.frames {
        let mut ids: Vec<&str> = f.positions.iter().map(|p| p.puuid.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), f.positions.len());
    }

    let plant = r.frames[1].marker.as_ref().unwrap();
    assert_eq!(plant.site.as_deref(), Some("A"));
    assert_eq!(plant.by.as_deref(), Some(A2));
    assert_eq!(r.attackers.as_deref(), Some("Red"));

    // Headshot only when every hit on the victim was a headshot.
    assert!(r.feed[0].kill.headshot);
    assert!(!r.feed[2].kill.headshot);

    // A round with no kills and no plant has no frames but still exists.
    assert!(replay.rounds[1].frames.is_empty());
    assert_eq!(replay.rounds[1].winning_team, "Blue");
}

/// Ascent-like constants of the shape valorant-api.com publishes (the app
/// reads the real ones live from /v1/maps). The transform swaps axes: image
/// x comes from world y, image y from world x.
fn ascent() -> MapCal {
    MapCal::new(7e-5, -7e-5, 0.813895, 0.573242).unwrap()
}

#[test]
fn world_to_minimap_swaps_axes() {
    let c = ascent();
    let (nx, ny) = world_to_norm(&Loc { x: 0.0, y: 0.0 }, &c);
    assert!((nx - 0.813895).abs() < 1e-6 && (ny - 0.573242).abs() < 1e-6);
    // Moving along world +y moves right on the image; world +x moves up.
    let (nx2, ny2) = world_to_norm(&Loc { x: 1000.0, y: -5000.0 }, &c);
    assert!((nx2 - (0.813895 - 0.35)).abs() < 1e-5, "{nx2}");
    assert!((ny2 - (0.573242 - 0.07)).abs() < 1e-5, "{ny2}");
    // The fixture's plant lands inside the image.
    let (px, py) = world_to_norm(&Loc { x: 6600.0, y: -7060.0 }, &c);
    assert!((0.0..=1.0).contains(&px) && (0.0..=1.0).contains(&py), "{px},{py}");
}

#[test]
fn facing_follows_the_same_axis_swap() {
    let c = ascent();
    // Facing world +x (θ = 0) points up on the image (y multiplier < 0).
    let (dx, dy) = facing(0.0, &c);
    assert!(dx.abs() < 1e-5 && (dy + 1.0).abs() < 1e-5, "{dx},{dy}");
    // Facing world +y (θ = π/2) points right.
    let (dx, dy) = facing(std::f32::consts::FRAC_PI_2, &c);
    assert!((dx - 1.0).abs() < 1e-5 && dy.abs() < 1e-5, "{dx},{dy}");
}

#[test]
fn deathmatch_has_no_replay() {
    assert_eq!(
        availability_for(
            "deathmatch",
            "/Game/GameModes/Deathmatch/DeathmatchGameMode.DeathmatchGameMode_C"
        ),
        Availability::UnsupportedMode("Deathmatch".into())
    );
    assert_eq!(
        availability_for("ggteam", ""),
        Availability::UnsupportedMode("Escalation".into())
    );
    assert_eq!(
        availability_for("competitive", "/Game/GameModes/Bomb/BombGameMode.BombGameMode_C"),
        Availability::Available
    );
    assert_eq!(availability_for("spikerush", ""), Availability::Available);
}

#[test]
fn kills_without_any_positions_mean_feed_only() {
    let mut raw = fixture();
    for r in &mut raw.round_results {
        r.plant_player_locations = None;
        r.bomb_planter = None;
        for ps in &mut r.player_stats {
            for k in &mut ps.kills {
                k.player_locations.clear();
            }
        }
    }
    let replay = from_raw(&raw);
    assert_eq!(replay.availability, Availability::NoPositions);
    assert_eq!(replay.rounds[0].feed.len(), 3);
}

#[test]
fn every_round_of_a_full_match_holds_together() {
    // The demo generator emits valapidocs-shaped JSON for a whole match.
    let m = tracked::worker::demo::synthetic_replay(3);
    assert_eq!(m.availability, Availability::Available);
    assert!(m.rounds.len() >= 13);
    for r in &m.rounds {
        // Time-ordered, and every positional kill in the feed points at a kill frame.
        assert!(r.frames.windows(2).all(|w| w[0].t_ms <= w[1].t_ms), "round {}", r.num);
        for e in &r.feed {
            if let Some(i) = e.frame {
                assert_eq!(r.frames[i].kind, FrameKind::Kill);
                assert_eq!(r.frames[i].kill.as_ref(), Some(&e.kill));
            }
        }
        // Once dead, never alive again later in the round.
        let mut dead = std::collections::HashSet::new();
        for f in &r.frames {
            for p in &f.positions {
                if p.alive {
                    assert!(!dead.contains(&p.puuid), "round {} revived {}", r.num, p.puuid);
                } else {
                    dead.insert(p.puuid.clone());
                }
            }
        }
        // A defuse only ever follows a plant.
        let kinds: Vec<FrameKind> = r
            .frames
            .iter()
            .map(|f| f.kind)
            .filter(|k| *k != FrameKind::Kill)
            .collect();
        assert!(
            kinds.is_empty() || kinds[0] == FrameKind::Plant,
            "round {}: {kinds:?}",
            r.num
        );
    }
}

#[test]
fn playback_is_exact_at_recorded_moments_and_smooth_between() {
    let replay = from_raw(&fixture());
    let tl = &replay.rounds[0].timeline;
    let pose = |t: f32, who: &str| tl.poses_at(t).into_iter().find(|p| p.puuid == who).unwrap();

    // At the 22s kill A1 is exactly where the API put them, and B3 is down.
    let a1 = pose(22000.0, A1);
    assert_eq!(a1.pos, Loc { x: 2000.0, y: -6500.0 });
    assert!(a1.alive && a1.exact);
    assert!(!pose(22000.0, B3).alive);
    // At the plant (41s), the plant snapshot.
    assert_eq!(pose(41000.0, A1).pos, Loc { x: 6100.0, y: -7400.0 });
    // Between them: on the way, and marked as not exact.
    let mid = pose(31500.0, A1);
    assert!(!mid.exact);
    assert!(mid.pos.x > 2000.0 && mid.pos.x < 6100.0, "{:?}", mid.pos);
    // A2 dies at 47s in a kill with no positions: alive before, down after,
    // where they were last seen.
    assert!(pose(46000.0, A2).alive);
    let a2 = pose(47000.0, A2);
    assert!(!a2.alive);
    assert_eq!(a2.pos, Loc { x: 6550.0, y: -7000.0 });
    // The timeline starts a little before the first event and ends after the last.
    assert!(tl.start < 22000.0 && tl.end > 62000.0);
}

#[test]
fn playback_matches_every_snapshot_of_a_full_match() {
    let m = tracked::worker::demo::synthetic_replay(5);
    for r in &m.rounds {
        let tl = &r.timeline;
        for f in &r.frames {
            let poses = tl.poses_at(f.t_ms as f32);
            for p in f.positions.iter().filter(|p| p.alive) {
                let got = poses.iter().find(|x| x.puuid == p.puuid).expect("pose");
                assert!(got.alive, "round {} t {}: {} should be alive", r.num, f.t_ms, p.puuid);
                assert!((got.pos.x - p.pos.x).abs() < 0.5 && (got.pos.y - p.pos.y).abs() < 0.5);
            }
        }
        // Sweep the round: nobody comes back after dying.
        let mut dead = std::collections::HashSet::new();
        let mut t = tl.start;
        while t <= tl.end {
            for p in tl.poses_at(t) {
                if p.alive {
                    assert!(!dead.contains(p.puuid), "round {} t {t}: {} revived", r.num, p.puuid);
                } else {
                    dead.insert(p.puuid.to_string());
                }
            }
            t += 250.0;
        }
    }
}

#[test]
fn downloaded_replays_are_matched_by_id() {
    use tracked::replay::vrf;
    let root = std::env::temp_dir().join(format!("tracked-vrf-{}", std::process::id()));
    let demos = root.join("VALORANT").join("Saved").join("Demos");
    std::fs::create_dir_all(&demos).unwrap();
    std::env::set_var("LOCALAPPDATA", &root);

    // One file named after its match, one whose Unreal replay name carries it.
    let by_name = "11111111-2222-4333-8444-555555555555";
    std::fs::write(demos.join(format!("{by_name}.vrf")), b"anything").unwrap();
    let by_header = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let mut v = b"RIOT".to_vec();
    for x in [vrf::UE_REPLAY_MAGIC, 6, 1000, 1, 1] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    let name = format!("{by_header}\0");
    v.extend_from_slice(&(name.len() as u32).to_le_bytes());
    v.extend_from_slice(name.as_bytes());
    v.extend_from_slice(&[0u8; 32]);
    std::fs::write(demos.join("replay2.vrf"), v).unwrap();

    let file = |id: &str| vrf::find_for_match(id).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    assert_eq!(file(by_name).as_deref(), Some(&*format!("{by_name}.vrf")));
    assert_eq!(file(by_header).as_deref(), Some("replay2.vrf"));
    assert_eq!(file("99999999-0000-4000-8000-000000000000"), None);
    assert!(vrf::check_all(None).contains("replay file check"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replay_file_items_land_in_their_round_on_the_api_clock() {
    use tracked::replay::vrf_spawns::{attach, Spawn, Stream};
    use tracked::replay::ItemKind;
    let mut replay = from_raw(&fixture());
    let spawn = |t_ms, end_ms, arch: &str, pkg: &str| Spawn {
        t_ms,
        archetype: arch.into(),
        package: pkg.into(),
        pos: [100.0, -200.0, 50.0],
        end_ms,
    };
    let smoke = (
        "Default__GameObject_Smonk_NewSmoke_C",
        "/Game/Characters/Smonk/S0/Ability_E/GameObject_Smonk_NewSmoke",
    );
    let drop = (
        "Default__EquippableGroundPickup_C",
        "/Game/Weapons/WeaponPickups/EquippableGroundPickup",
    );
    // Round 0 starts at 10 s of replay time; the file's clock runs 30 s
    // ahead of the API's round clock (buy phase). Kills at 22/47/62 s.
    let stream = Stream {
        round_starts: vec![10_000, 200_000],
        deaths: vec![62_000, 87_000, 102_000],
        spawns: vec![
            spawn(55_000, Some(70_000), smoke.0, smoke.1),
            spawn(80_000, None, drop.0, drop.1),
            spawn(15_000, Some(20_000), smoke.0, smoke.1), // gone before round time 0
            spawn(
                90_000,
                None,
                "Default__AssaultRifle_AK_C",
                "/Game/Equippables/Guns/Rifles/AK/AssaultRifle_AK",
            ),
            spawn(210_000, None, drop.0, drop.1), // next round
        ],
        ..Default::default()
    };
    attach(&mut replay, &stream);
    let r0 = &replay.rounds[0].items;
    assert_eq!(r0.len(), 2, "{r0:?}");
    assert_eq!((r0[0].t, r0[0].end), (15_000.0, Some(30_000.0)));
    assert!(matches!(&r0[0].kind, ItemKind::Ability { agent, .. } if agent == "Smonk"));
    assert_eq!(r0[0].pos, Loc { x: 100.0, y: -200.0 });
    assert_eq!(
        (r0[1].kind.clone(), r0[1].t, r0[1].end),
        (ItemKind::DroppedWeapon, 40_000.0, None)
    );
    // Round 1 has no kills: it uses the match's typical offset.
    let r1 = &replay.rounds[1].items;
    assert_eq!(r1.len(), 1);
    // 10 s into the file's round minus the 30 s offset: clamped to round start.
    assert_eq!(r1[0].t, 0.0);
}
