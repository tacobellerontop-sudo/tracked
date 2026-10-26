//! PUUID <-> Riot ID resolution with an in-memory cache.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;

use super::{RiotId, Session};
use crate::error::ApiError;

/// Riot rejects very large batches; stay well under the limit.
const BATCH: usize = 50;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NameEntry {
    subject: String,
    game_name: String,
    tag_line: String,
}

#[derive(Default)]
pub struct NameCache {
    by_puuid: Mutex<HashMap<String, RiotId>>,
}

impl NameCache {
    pub fn insert(&self, puuid: &str, id: RiotId) {
        if !id.game_name.is_empty() {
            self.by_puuid.lock().unwrap().insert(puuid.to_string(), id);
        }
    }

    pub fn get(&self, puuid: &str) -> Option<RiotId> {
        self.by_puuid.lock().unwrap().get(puuid).cloned()
    }

    /// Reverse lookup by `name#tag`, case-insensitive. Only finds players we
    /// have already seen (friends, lobbies, match participants).
    pub fn find(&self, name: &str, tag: &str) -> Option<String> {
        self.by_puuid
            .lock()
            .unwrap()
            .iter()
            .find(|(_, id)| id.game_name.eq_ignore_ascii_case(name) && id.tag_line.eq_ignore_ascii_case(tag))
            .map(|(p, _)| p.clone())
    }

    /// Resolve PUUIDs, fetching only the ones not already cached.
    pub async fn resolve(&self, s: &Session, puuids: &[String]) -> Result<HashMap<String, RiotId>, ApiError> {
        let missing: Vec<String> = {
            let cache = self.by_puuid.lock().unwrap();
            let mut m: Vec<String> = puuids.iter().filter(|p| !cache.contains_key(*p)).cloned().collect();
            m.sort();
            m.dedup();
            m
        };
        for chunk in missing.chunks(BATCH) {
            for e in fetch(s, chunk).await? {
                self.insert(
                    &e.subject,
                    RiotId {
                        game_name: e.game_name,
                        tag_line: e.tag_line,
                    },
                );
            }
        }
        let cache = self.by_puuid.lock().unwrap();
        Ok(puuids
            .iter()
            .filter_map(|p| cache.get(p).map(|id| (p.clone(), id.clone())))
            .collect())
    }
}

/// One request; if Riot says the batch is too big, split it in half.
async fn fetch(s: &Session, puuids: &[String]) -> Result<Vec<NameEntry>, ApiError> {
    let url = format!("{}/name-service/v2/players", s.pd()?);
    match s.put::<_, Vec<NameEntry>>(&url, &puuids).await {
        Err(ApiError::Status(400)) if puuids.len() > 1 => {
            let (a, b) = puuids.split_at(puuids.len() / 2);
            let mut out = Box::pin(fetch(s, a)).await?;
            out.extend(Box::pin(fetch(s, b)).await?);
            Ok(out)
        }
        other => other,
    }
}
