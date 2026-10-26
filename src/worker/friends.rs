//! Built-in social: friends, presence and whispers through the Riot Client's
//! own chat service. Nothing here touches the optional Ascend backend, and
//! message contents are never logged.

use std::sync::Arc;

use super::tasks::Ctx;
use crate::error::UserFacingError;
use crate::ipc::{Intent, Update};
use crate::riot::chat::{self, FriendStatus};
use crate::riot::presence::RawPresence;
use crate::riot::RiotId;

/// Handle a built-in social intent. Returns `false` if it isn't one.
pub fn handle(ctx: &Arc<Ctx>, intent: &Intent) -> bool {
    let ctx = ctx.clone();
    match intent.clone() {
        Intent::RiotSocialRefresh => {
            tokio::spawn(refresh(ctx));
        }
        Intent::RiotChatOpen(cid) => {
            tokio::spawn(async move {
                let Ok(s) = ctx.session() else { return };
                match chat::history(&s.local, &cid).await {
                    Ok(messages) => ctx.sink.send(Update::RiotChatHistory { cid, messages }),
                    Err(e) => tracing::debug!("chat history failed: {e}"),
                }
            });
        }
        Intent::RiotChatSend { cid, text } => {
            tokio::spawn(async move {
                let result = match ctx.session() {
                    Ok(s) => chat::send(&s.local, &cid, &text).await,
                    Err(e) => Err(e),
                };
                match result {
                    Ok(sent) => ctx.sink.send(Update::RiotChat(sent)),
                    Err(e) => {
                        tracing::info!("whisper failed: {e}");
                        ctx.sink.send(Update::RiotChatFailed {
                            cid,
                            reason: UserFacingError::from(&e).0,
                        });
                    }
                }
            });
        }
        _ => return false,
    }
    true
}

pub async fn refresh(ctx: Arc<Ctx>) {
    let Ok(s) = ctx.session() else { return };
    let (friends, requests, convs, presences) = tokio::join!(
        chat::friends(&s.local),
        chat::friend_requests(&s.local),
        chat::conversations(&s.local),
        s.local.presences(),
    );
    match friends {
        Ok(friends) => {
            for f in &friends {
                ctx.names.insert(
                    &f.puuid,
                    RiotId {
                        game_name: f.game_name.clone(),
                        tag_line: f.game_tag.clone(),
                    },
                );
            }
            ctx.sink.send(Update::RiotFriends {
                friends,
                requests: requests.unwrap_or_default(),
            });
        }
        Err(e) => {
            tracing::warn!("friends list failed: {e}");
            ctx.report(&e);
        }
    }
    if let Ok(c) = convs {
        ctx.sink.send(Update::RiotConversations(c));
    }
    if let Ok(p) = presences {
        let me = s.puuid.clone();
        ctx.sink.send(Update::RiotStatuses(statuses(&p.presences, &me)));
    }
}

/// Decode presences of everyone but us.
pub fn statuses(list: &[RawPresence], me: &str) -> Vec<(String, FriendStatus)> {
    list.iter()
        .filter(|p| p.puuid != me)
        .map(|p| (p.puuid.clone(), chat::friend_status(p)))
        .collect()
}
