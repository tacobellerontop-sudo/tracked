use ascend_lite::riot::presence::{decode_private, presences_from_event, LoopState};
use base64::Engine;

fn b64(v: serde_json::Value) -> String {
    base64::engine::general_purpose::STANDARD.encode(v.to_string())
}

#[test]
fn decodes_flat_layout() {
    let blob = b64(serde_json::json!({
        "isValid": true, "sessionLoopState": "INGAME", "partyId": "p1", "partySize": 3,
        "queueId": "unrated", "matchMap": "/Game/Maps/Bonsai/Bonsai"
    }));
    let g = decode_private(&blob).unwrap();
    assert_eq!(g.loop_state, LoopState::InGame);
    assert_eq!(g.party_id, "p1");
    assert_eq!(g.party_size, 3);
    assert_eq!(g.queue_id, "unrated");
    assert_eq!(g.map, "/Game/Maps/Bonsai/Bonsai");
}

#[test]
fn decodes_nested_layout() {
    let blob = b64(serde_json::json!({
        "matchPresenceData": { "sessionLoopState": "PREGAME", "queueId": "competitive", "matchMap": "" },
        "partyPresenceData": { "partyId": "p2", "partySize": 5 }
    }));
    let g = decode_private(&blob).unwrap();
    assert_eq!(g.loop_state, LoopState::Pregame);
    assert_eq!(g.party_size, 5);
    assert_eq!(g.queue_id, "competitive");
}

#[test]
fn rejects_garbage() {
    assert!(decode_private("").is_none());
    assert!(decode_private("!!!not base64").is_none());
    assert!(decode_private(&b64(serde_json::json!({"foo": 1}))).is_none());
    assert_eq!(
        decode_private(&b64(serde_json::json!({"sessionLoopState": "WAT"})))
            .unwrap()
            .loop_state,
        LoopState::Unknown
    );
}

#[test]
fn parses_event_frames() {
    let frame = r#"[8,"OnJsonApiEvent_chat_v4_presences",{"data":{"presences":[{"puuid":"a","product":"valorant","private":"e30="}]},"eventType":"Update"}]"#;
    let list = presences_from_event(frame).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].puuid, "a");
    assert!(presences_from_event("").is_none());
    assert!(presences_from_event(r#"[5,"OnJsonApiEvent"]"#).is_none());
}
