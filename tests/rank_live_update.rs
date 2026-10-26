use ascend_lite::ipc::UpdateStatus;
use ascend_lite::riot::glz::{party_to_live, pregame_to_live, RawParty, RawPregame};
use ascend_lite::riot::models::{LivePhase, PlayerMmr};
use ascend_lite::riot::pd::{is_uuid, rank_snapshot};
use ascend_lite::worker::tasks::compare_release;
use serde_json::json;

fn mmr() -> PlayerMmr {
    serde_json::from_value(json!({
        "Subject": "x",
        "LatestCompetitiveUpdate": { "SeasonID": "old-act", "TierAfterUpdate": 18, "RankedRatingAfterUpdate": 55 },
        "QueueSkills": { "competitive": { "SeasonalInfoBySeasonID": {
            "old-act": { "SeasonID": "old-act", "CompetitiveTier": 18, "RankedRating": 55, "NumberOfWins": 30,
                         "NumberOfGames": 50, "WinsByTier": { "17": 10, "19": 2 } },
            "new-act": { "SeasonID": "new-act", "CompetitiveTier": 0, "NumberOfWins": 1, "NumberOfGames": 2 }
        } } }
    }))
    .unwrap()
}

#[test]
fn rank_uses_latest_update_when_act_unknown() {
    let r = rank_snapshot(&mmr(), None);
    assert_eq!((r.tier, r.rr, r.season_wins, r.season_games), (18, 55, 30, 50));
    assert_eq!(r.peak_tier, 19);
    assert_eq!(r.win_rate(), Some(60.0));
}

#[test]
fn rank_is_unrated_in_a_new_act() {
    let r = rank_snapshot(&mmr(), Some("new-act"));
    assert_eq!(r.tier, 0, "latest update is from an older act");
    assert_eq!(r.season_games, 2);
    assert_eq!(r.peak_tier, 19, "peak spans all acts");
}

#[test]
fn rank_of_empty_payload() {
    let r = rank_snapshot(&PlayerMmr::default(), Some("x"));
    assert_eq!(r.tier, 0);
    assert_eq!(r.win_rate(), None);
}

#[test]
fn update_comparison() {
    let rel = |tag: &str| json!({ "tag_name": tag, "html_url": "https://example.com/r" });
    assert!(
        matches!(compare_release(&rel("v0.2.0"), "0.1.0"), UpdateStatus::Available { ref version, .. } if version == "0.2.0")
    );
    assert_eq!(compare_release(&rel("0.1.0"), "0.1.0"), UpdateStatus::UpToDate);
    assert_eq!(compare_release(&rel("v0.0.9"), "0.1.0"), UpdateStatus::UpToDate);
    assert!(matches!(
        compare_release(&rel("nightly"), "0.1.0"),
        UpdateStatus::Failed(_)
    ));
}

#[test]
fn pregame_normalization_respects_privacy_flags() {
    let raw: RawPregame = serde_json::from_value(json!({
        "ID": "m", "MapID": "/Game/Maps/Ascent/Ascent", "QueueID": "competitive",
        "AllyTeam": { "TeamID": "Red", "Players": [
            { "Subject": "me", "CharacterID": "ABC", "CharacterSelectionState": "locked",
              "PlayerIdentity": { "AccountLevel": 50, "Incognito": true, "HideAccountLevel": false } },
            { "Subject": "them", "CharacterID": "", "CharacterSelectionState": "",
              "PlayerIdentity": { "AccountLevel": 99, "Incognito": true, "HideAccountLevel": true } }
        ] }
    }))
    .unwrap();
    let live = pregame_to_live(&raw, "me");
    assert_eq!(live.phase, LivePhase::Pregame);
    let me = &live.players[0];
    assert!(
        me.is_self && me.locked && !me.incognito,
        "own streamer mode doesn't hide us from ourselves"
    );
    assert_eq!(me.character_id, "abc");
    let them = &live.players[1];
    assert!(them.incognito && !them.locked);
    assert_eq!(them.account_level, None);
}

#[test]
fn party_normalization() {
    let raw: RawParty = serde_json::from_value(json!({
        "ID": "p", "MatchmakingData": { "QueueID": "unrated" },
        "Members": [{ "Subject": "me" }, { "Subject": "friend", "PlayerIdentity": { "AccountLevel": 12 } }]
    }))
    .unwrap();
    let live = party_to_live(&raw, "me");
    assert_eq!(live.phase, LivePhase::Party);
    assert_eq!(live.queue_id, "unrated");
    assert_eq!(live.teams(), vec!["Party".to_string()]);
    assert_eq!(live.players[1].account_level, Some(12));
}

#[test]
fn uuid_validation_blocks_path_injection() {
    assert!(is_uuid("0b0e4a2c-1111-4b6c-9c6e-123456789abc"));
    assert!(!is_uuid("../../etc/passwd"));
    assert!(!is_uuid("0b0e4a2c-1111-4b6c-9c6e-123456789ab/"));
    assert!(!is_uuid(""));
}
