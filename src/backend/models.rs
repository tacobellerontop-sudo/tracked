//! Wire types for the backend REST API and social websocket.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BackendUser {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub riot_id: Option<String>,
    #[serde(default)]
    pub bot: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Features {
    pub social: bool,
    pub dms: bool,
    pub lobbies: bool,
    pub screenshare: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub features: Features,
    pub motd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MinVersion {
    pub min_version: String,
    #[serde(default)]
    pub download_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MinVersionState {
    Ok,
    /// The running version is below the backend's minimum: the app locks itself.
    Blocked {
        required: String,
        download_url: Option<String>,
    },
}

impl MinVersionState {
    pub fn evaluate(min: &MinVersion, current: &str) -> Self {
        match (
            semver::Version::parse(min.min_version.trim_start_matches('v')),
            semver::Version::parse(current),
        ) {
            (Ok(required), Ok(cur)) if cur < required => Self::Blocked {
                required: required.to_string(),
                download_url: min.download_url.clone(),
            },
            // An unparsable minimum never locks the user out.
            _ => Self::Ok,
        }
    }
}

/// What we publish about our party. Deliberately no party id or member list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct LobbyInfo {
    pub queue: String,
    pub size: u32,
    pub max: u32,
    pub open: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PresenceEntry {
    pub user_id: String,
    pub online: bool,
    pub state: Option<String>,
    pub queue: Option<String>,
    pub party_size: Option<u32>,
    pub using_app: bool,
    pub lobby: Option<LobbyInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DmMessage {
    pub id: u64,
    pub conversation_id: String,
    pub sender_id: String,
    pub text: String,
    pub sent_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Conversation {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub last_message: Option<DmMessage>,
    #[serde(default)]
    pub unread: u32,
    #[serde(default)]
    pub is_group: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DmPage {
    pub messages: Vec<DmMessage>,
    pub has_more: bool,
}
