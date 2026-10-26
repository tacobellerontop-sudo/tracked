//! Phase 3 gate: the real worker against the in-process mock backend.
//! Sign in, friends + presence + lobbies, DMs over REST, bot replies and
//! typing over the social websocket, group create/rename, feedback, and the
//! min-version gate.

use std::time::{Duration, Instant};

use ascend_lite::backend::models::MinVersionState;
use ascend_lite::config::Settings;
use ascend_lite::ipc::{self, BackendStatus, Intent, Update, UpdateRx};

fn wait_for<T>(rx: &UpdateRx, secs: u64, mut f: impl FnMut(&Update) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if let Ok(u) = rx.recv_timeout(Duration::from_millis(100)) {
            if let Some(t) = f(&u) {
                return t;
            }
        }
    }
    panic!("timed out waiting for update");
}

fn start_mock(rt: &tokio::runtime::Runtime) -> String {
    let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let addr = listener.local_addr().unwrap();
    rt.spawn(mock_backend::serve(listener));
    format!("http://{addr}")
}

#[test]
fn social_against_mock_backend() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let url = start_mock(&rt);
    std::env::set_var("ASCEND_LOCKFILE", "/nonexistent/ascend/lockfile");
    std::env::set_var(
        "ASCEND_CONFIG_DIR",
        std::env::temp_dir().join(format!("ascend-social-{}", std::process::id())),
    );

    let (tx, rx, sink, updates) = ipc::channels(egui::Context::default());
    let settings = Settings {
        backend_url: Some(url),
        check_for_updates: false,
        ..Default::default()
    };
    let handle = ascend_lite::worker::spawn(rx, sink, settings);

    wait_for(&updates, 5, |u| {
        matches!(u, Update::Backend(BackendStatus::SignedOut)).then_some(())
    });
    let gate = wait_for(&updates, 5, |u| match u {
        Update::MinVersion(m) => Some(m.clone()),
        _ => None,
    });
    assert_eq!(gate, MinVersionState::Ok);

    tx.send(Intent::BackendSignIn {
        display_name: "Tester".into(),
    });
    let me = wait_for(&updates, 5, |u| match u {
        Update::Backend(BackendStatus::SignedIn(user)) => Some(user.clone()),
        _ => None,
    });
    assert_eq!(me.display_name, "Tester");

    let friends = wait_for(&updates, 5, |u| match u {
        Update::SocialFriends(f) => Some(f.clone()),
        _ => None,
    });
    assert_eq!(friends.len(), 3);
    let presence = wait_for(&updates, 5, |u| match u {
        Update::SocialPresence(p) if p.len() == 3 => Some(p.clone()),
        _ => None,
    });
    let nimbus = presence.iter().find(|p| p.user_id == "bot-nimbus").unwrap();
    assert!(nimbus.online && nimbus.state.as_deref() == Some("INGAME"));
    assert!(!presence.iter().find(|p| p.user_id == "bot-orchid").unwrap().online);
    let lobby = wait_for(&updates, 5, |u| match u {
        Update::SocialLobby {
            user_id,
            lobby: Some(l),
        } if user_id == "bot-kettle" => Some(l.clone()),
        _ => None,
    });
    assert_eq!(lobby.size, 2);
    let convs = wait_for(&updates, 5, |u| match u {
        Update::DmConversations(c) => Some(c.clone()),
        _ => None,
    });
    let dm = convs.iter().find(|c| c.title == "Nimbus").expect("seeded DM").clone();
    assert_eq!(dm.unread, 1);
    wait_for(&updates, 5, |u| matches!(u, Update::SocialLive(true)).then_some(()));

    // Page, send, and get the bot's typing indicator + reply over the socket.
    tx.send(Intent::DmOpen(dm.id.clone()));
    let page = wait_for(&updates, 5, |u| match u {
        Update::DmPage {
            conversation_id,
            messages,
            ..
        } if *conversation_id == dm.id => Some(messages.clone()),
        _ => None,
    });
    assert_eq!(page.len(), 1);
    tx.send(Intent::DmSend {
        conversation_id: dm.id.clone(),
        text: "hello bot".into(),
    });
    let typing = wait_for(&updates, 5, |u| match u {
        Update::DmTyping { user_id, .. } => Some(user_id.clone()),
        _ => None,
    });
    assert_eq!(typing, "bot-nimbus");
    let reply = wait_for(&updates, 5, |u| match u {
        Update::DmMessage(m) if m.sender_id == "bot-nimbus" => Some(m.text.clone()),
        _ => None,
    });
    assert!(reply.contains("hello bot"), "{reply}");

    // Group chat create + rename.
    tx.send(Intent::DmCreate {
        members: vec!["bot-kettle".into(), "bot-orchid".into()],
        name: Some("squad".into()),
    });
    let group = wait_for(&updates, 5, |u| match u {
        Update::DmConversation(c) if c.is_group => Some(c.clone()),
        _ => None,
    });
    assert_eq!(group.title, "squad");
    tx.send(Intent::DmRename {
        conversation_id: group.id.clone(),
        name: "the squad".into(),
    });
    wait_for(&updates, 5, |u| match u {
        Update::DmConversation(c) if c.id == group.id && c.title == "the squad" => Some(()),
        _ => None,
    });

    tx.send(Intent::SendFeedback("works great".into()));
    wait_for(&updates, 5, |u| matches!(u, Update::Notice(_)).then_some(()));

    tx.send(Intent::BackendSignOut);
    wait_for(&updates, 5, |u| {
        matches!(u, Update::Backend(BackendStatus::SignedOut)).then_some(())
    });
    tx.send(Intent::Shutdown);
    handle.join().unwrap();
}

#[test]
fn min_version_gate_blocks_old_clients() {
    use ascend_lite::backend::models::MinVersion;
    let min = |v: &str| MinVersion {
        min_version: v.into(),
        download_url: Some("https://x".into()),
    };
    assert_eq!(MinVersionState::evaluate(&min("0.1.0"), "0.1.0"), MinVersionState::Ok);
    assert_eq!(MinVersionState::evaluate(&min("v0.0.5"), "0.1.0"), MinVersionState::Ok);
    assert!(matches!(
        MinVersionState::evaluate(&min("0.2.0"), "0.1.0"),
        MinVersionState::Blocked { .. }
    ));
    assert_eq!(
        MinVersionState::evaluate(&min("garbage"), "0.1.0"),
        MinVersionState::Ok,
        "never lock out on bad data"
    );
}

#[test]
fn unauthenticated_calls_are_rejected() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let url = start_mock(&rt);
    let client = ascend_lite::backend::BackendClient::new(&url).unwrap();
    let err = rt.block_on(client.friends()).unwrap_err();
    assert!(matches!(err, ascend_lite::error::ApiError::Unauthorized));
    // Public endpoints work without a token.
    assert!(rt.block_on(client.min_version()).is_ok());
}
