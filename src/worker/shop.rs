//! Store and Loadout intents: fetch the raw payloads (read-only PD calls),
//! join them with valorant-api cosmetics and publish the result.

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::OnceCell;

use super::tasks::Ctx;
use crate::content::cosmetics::{self, Cosmetics};
use crate::error::{ApiError, UserFacingError};
use crate::ipc::{Intent, Update};
use crate::riot::store::{self, ids};
use crate::shop::{self, LoadoutView, Owned, StoreView};

/// Cosmetics are big and never change while the app runs: fetch them once.
static COSMETICS: OnceCell<Arc<Cosmetics>> = OnceCell::const_new();

/// Handle store / loadout intents; returns false for anything else.
pub fn handle(ctx: &Arc<Ctx>, intent: &Intent) -> bool {
    match intent {
        Intent::RefreshStore => {
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let r = refresh_store(&ctx).await.map_err(|e| {
                    tracing::warn!("store: {e}");
                    UserFacingError::from(&e)
                });
                ctx.sink.send(Update::Store(r.map(Arc::new)));
            });
            true
        }
        Intent::RefreshLoadout => {
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let r = refresh_loadout(&ctx).await.map_err(|e| {
                    tracing::warn!("loadout: {e}");
                    UserFacingError::from(&e)
                });
                ctx.sink.send(Update::Loadout(r.map(Arc::new)));
            });
            true
        }
        _ => false,
    }
}

/// Names and art; empty (ids only) when offline or valorant-api is down.
async fn cosmetics(ctx: &Ctx) -> Arc<Cosmetics> {
    if std::env::var_os("TRACKED_OFFLINE_CONTENT").is_some() {
        return Arc::default();
    }
    let r = COSMETICS
        .get_or_try_init(|| async { cosmetics::fetch(&ctx.http).await.map(Arc::new) })
        .await;
    match r {
        Ok(c) => c.clone(),
        Err(e) => {
            // Not cached, so the next refresh tries again.
            tracing::warn!("couldn't load cosmetics from valorant-api.com: {e}");
            Arc::default()
        }
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub async fn refresh_store(ctx: &Arc<Ctx>) -> Result<StoreView, ApiError> {
    let s = ctx.session()?;
    let (front, wallet, owned, cos) = tokio::join!(
        store::storefront(&s, &s.puuid),
        store::wallet(&s, &s.puuid),
        store::owned(&s, &s.puuid, ids::SKIN_LEVEL),
        cosmetics(ctx),
    );
    let front = front?;
    let wallet = wallet.unwrap_or_else(|e| {
        tracing::warn!("wallet: {e}");
        Default::default()
    });
    let owned: HashSet<String> = owned.unwrap_or_default().into_iter().collect();
    Ok(shop::build_store(&front, &wallet, &owned, &cos, now()))
}

pub async fn refresh_loadout(ctx: &Arc<Ctx>) -> Result<LoadoutView, ApiError> {
    let s = ctx.session()?;
    let p = s.puuid.as_str();
    let (lo, xp, skins, buddies, sprays, cards, titles, cos) = tokio::join!(
        store::loadout(&s, p),
        store::account_xp(&s, p),
        store::owned(&s, p, ids::SKIN_LEVEL),
        store::owned(&s, p, ids::BUDDY),
        store::owned(&s, p, ids::SPRAY),
        store::owned(&s, p, ids::CARD),
        store::owned(&s, p, ids::TITLE),
        cosmetics(ctx),
    );
    let lo = lo?;
    let xp = xp.map_err(|e| tracing::warn!("account xp: {e}")).ok();
    let owned = Owned {
        skin_levels: skins.ok(),
        buddies: buddies.ok(),
        sprays: sprays.ok(),
        cards: cards.ok(),
        titles: titles.ok(),
    };
    Ok(shop::build_loadout(&lo, xp.as_ref(), &owned, &cos, now()))
}

// ---------------------------------------------------------------- demo mode

/// `--demo`: synthetic payloads run through the same builders as real data.
pub mod demo {
    use std::collections::HashSet;

    use serde_json::json;

    use crate::content::cosmetics::{BundleInfo, ContentTier, Cosmetics, CurrencyInfo, Item, WeaponInfo};
    use crate::riot::store::ids;
    use crate::shop::{self, LoadoutView, Owned, StoreView};

    const TIERS: &[(&str, &str, u32)] = &[
        ("Select", "5a9fe2ff", 0),
        ("Deluxe", "009587ff", 1),
        ("Premium", "d1548dff", 2),
        ("Exclusive", "f5955bff", 3),
        ("Ultra", "fad663ff", 4),
    ];
    /// (name, order, cost)
    const WEAPONS: &[(&str, u32, u32)] = &[
        ("Classic", 0, 0),
        ("Shorty", 0, 300),
        ("Frenzy", 0, 450),
        ("Ghost", 0, 500),
        ("Sheriff", 0, 800),
        ("Stinger", 1, 1100),
        ("Spectre", 1, 1600),
        ("Bucky", 2, 850),
        ("Judge", 2, 1850),
        ("Bulldog", 3, 2050),
        ("Guardian", 3, 2250),
        ("Phantom", 3, 2900),
        ("Vandal", 3, 2900),
        ("Marshal", 4, 950),
        ("Outlaw", 4, 2400),
        ("Operator", 4, 4700),
        ("Ares", 5, 1600),
        ("Odin", 5, 3200),
        ("Melee", 7, 0),
    ];
    const LINES: &[&str] = &[
        "Aurora",
        "Glasswork",
        "Ember Tide",
        "Monolith",
        "Static Bloom",
        "Driftwood",
        "Neon Koi",
        "Obsidian",
    ];

    fn uuid(kind: u32, n: usize) -> String {
        format!("{kind:08x}-0000-4000-8000-{n:012x}")
    }
    fn weapon_id(w: usize) -> String {
        uuid(0xa0, w)
    }
    fn skin_id(line: usize, w: usize) -> String {
        uuid(0xa1, line * 100 + w)
    }
    fn level_id(line: usize, w: usize) -> String {
        uuid(0xa2, line * 100 + w)
    }
    fn chroma_id(line: usize, w: usize, c: usize) -> String {
        uuid(0xa3, line * 1000 + w * 10 + c)
    }
    fn tier_id(t: usize) -> String {
        uuid(0xa4, t)
    }
    fn buddy_id(n: usize) -> String {
        uuid(0xa5, n)
    }
    fn spray_id(n: usize) -> String {
        uuid(0xa6, n)
    }
    fn bundle_id(n: usize) -> String {
        uuid(0xa7, n)
    }

    fn cosmetics() -> Cosmetics {
        let mut c = Cosmetics::default();
        for (t, (name, color, rank)) in TIERS.iter().enumerate() {
            c.tiers.insert(
                tier_id(t),
                ContentTier {
                    name: name.to_string(),
                    color: color.to_string(),
                    icon: String::new(),
                    rank: *rank,
                },
            );
        }
        for (w, (name, order, cost)) in WEAPONS.iter().enumerate() {
            c.weapons.insert(
                weapon_id(w),
                WeaponInfo {
                    name: name.to_string(),
                    icon: String::new(),
                    order: *order,
                    cost: *cost,
                },
            );
            for (l, line) in LINES.iter().enumerate() {
                let item = |n: String| Item {
                    name: n,
                    tier: Some(tier_id(l % TIERS.len())),
                    skin: Some(skin_id(l, w)),
                    weapon: Some(weapon_id(w)),
                    ..Default::default()
                };
                let skin_name = format!("{line} {name}");
                c.items.insert(skin_id(l, w), item(skin_name.clone()));
                c.items.insert(level_id(l, w), item(skin_name.clone()));
                c.items.insert(chroma_id(l, w, 0), item(skin_name.clone()));
                c.items
                    .insert(chroma_id(l, w, 1), item(format!("{skin_name} Variant 1 Crimson")));
            }
        }
        for (n, name) in ["Lucky Die", "Tiny Spike", "Paper Crane", "Rubber Duck", "Ramen Bowl"]
            .iter()
            .enumerate()
        {
            c.items.insert(
                buddy_id(n),
                Item {
                    name: format!("{name} Buddy"),
                    ..Default::default()
                },
            );
        }
        for (n, name) in ["GG EZ", "Nice Try", "Clutch Mode", "Spike Planted"].iter().enumerate() {
            c.items.insert(
                spray_id(n),
                Item {
                    name: format!("{name} Spray"),
                    ..Default::default()
                },
            );
        }
        c.items.insert(
            uuid(0xa8, 1),
            Item {
                name: "Lobby Scout Card".into(),
                ..Default::default()
            },
        );
        c.items.insert(
            uuid(0xa9, 1),
            Item {
                name: "Lobby Scout".into(),
                ..Default::default()
            },
        );
        for (n, name) in ["Glasswork", "Ember Tide"].iter().enumerate() {
            c.bundles.insert(
                bundle_id(n),
                BundleInfo {
                    name: name.to_string(),
                    art: String::new(),
                },
            );
        }
        for (id, name) in [
            (ids::VP, "VALORANT Points"),
            (ids::RADIANITE, "Radianite Points"),
            (ids::KINGDOM_CREDITS, "Kingdom Credits"),
        ] {
            c.currencies.insert(
                id.to_string(),
                CurrencyInfo {
                    name: name.into(),
                    icon: String::new(),
                },
            );
        }
        c
    }

    fn price(line: usize) -> u32 {
        [875, 1275, 1775, 2175, 2475][line % 5]
    }

    fn offer(line: usize, w: usize) -> serde_json::Value {
        json!({ "OfferID": level_id(line, w), "Cost": { ids::VP: price(line) },
                "Rewards": [{ "ItemTypeID": ids::SKIN_LEVEL, "ItemID": level_id(line, w), "Quantity": 1 }] })
    }

    fn accessory(type_id: &str, item: String, kc: u32) -> serde_json::Value {
        json!({ "Offer": { "OfferID": item, "Cost": { ids::KINGDOM_CREDITS: kc },
                "Rewards": [{ "ItemTypeID": type_id, "ItemID": item, "Quantity": 1 }] }, "ContractID": "" })
    }

    pub fn store(now: i64) -> StoreView {
        // Resets line up with the real ones: daily at 00:00 UTC.
        let to_midnight = 86_400 - now.rem_euclid(86_400);
        let bundle_items = |line: usize, ws: &[usize]| {
            ws.iter()
                .map(|w| {
                    let base = price(line);
                    json!({ "Item": { "ItemTypeID": ids::SKIN_LEVEL, "ItemID": level_id(line, *w), "Amount": 1 },
                            "BasePrice": base, "CurrencyID": ids::VP, "DiscountPercent": 0.33,
                            "DiscountedPrice": base * 67 / 100 })
                })
                .chain([json!({ "Item": { "ItemTypeID": ids::BUDDY, "ItemID": buddy_id(2), "Amount": 1 },
                                "BasePrice": 475, "CurrencyID": ids::VP, "DiscountPercent": 0.0, "DiscountedPrice": 475 })])
                .collect::<Vec<_>>()
        };
        let night: Vec<_> = [
            (6, 11, 32, true),
            (7, 15, 41, true),
            (2, 9, 27, true),
            (5, 3, 18, false),
            (0, 13, 45, false),
            (4, 7, 22, false),
        ]
        .iter()
        .map(|(l, w, d, seen)| {
            json!({ "BonusOfferID": level_id(*l, *w), "Offer": offer(*l, *w), "DiscountPercent": d,
                        "DiscountCosts": { ids::VP: price(*l) * (100 - d) / 100 }, "IsSeen": seen })
        })
        .collect();
        let front = json!({
            "FeaturedBundle": {
                "Bundles": [
                    { "ID": "b1", "DataAssetID": bundle_id(0), "CurrencyID": ids::VP,
                      "Items": bundle_items(1, &[12, 6, 4, 18]),
                      "TotalBaseCost": { ids::VP: 7600 }, "TotalDiscountedCost": { ids::VP: 5100 },
                      "DurationRemainingInSeconds": 5 * 86_400 + 3 * 3600 },
                    { "ID": "b2", "DataAssetID": bundle_id(1), "CurrencyID": ids::VP,
                      "Items": bundle_items(2, &[11, 15, 3, 18]),
                      "TotalBaseCost": { ids::VP: 9900 }, "TotalDiscountedCost": { ids::VP: 7100 },
                      "DurationRemainingInSeconds": 2 * 86_400 + 9 * 3600 }
                ],
                "BundleRemainingDurationInSeconds": 2 * 86_400 + 9 * 3600
            },
            "SkinsPanelLayout": {
                "SingleItemStoreOffers": [offer(3, 12), offer(0, 4), offer(5, 6), offer(4, 18)],
                "SingleItemOffersRemainingDurationInSeconds": to_midnight
            },
            "BonusStore": {
                "BonusStoreOffers": night,
                "BonusStoreRemainingDurationInSeconds": 9 * 86_400 + 4 * 3600
            },
            "AccessoryStore": {
                "AccessoryStoreOffers": [
                    accessory(ids::BUDDY, buddy_id(3), 3000),
                    accessory(ids::SPRAY, spray_id(2), 2000),
                    accessory(ids::CARD, uuid(0xa8, 1), 2500),
                    accessory(ids::TITLE, uuid(0xa9, 1), 1500)
                ],
                "AccessoryStoreRemainingDurationInSeconds": 3 * 86_400 + 7 * 3600
            }
        });
        let wallet = json!({ "Balances": { ids::VP: 2340, ids::RADIANITE: 120, ids::KINGDOM_CREDITS: 8650, ids::FREE_AGENTS: 2 } });
        let owned: HashSet<String> = [level_id(1, 12), level_id(1, 18)].into_iter().collect();
        shop::build_store(
            &serde_json::from_value(front).unwrap_or_default(),
            &serde_json::from_value(wallet).unwrap_or_default(),
            &owned,
            &cosmetics(),
            now,
        )
    }

    pub fn loadout(now: i64) -> LoadoutView {
        let guns: Vec<_> = (0..WEAPONS.len())
            .map(|w| {
                // A mix of skinned and default guns, some with buddies.
                let line = [0, 9, 9, 2, 6, 9, 3, 9, 9, 9, 9, 1, 3, 9, 7, 5, 9, 9, 4][w];
                if line == 9 {
                    return json!({ "ID": weapon_id(w), "SkinID": "", "SkinLevelID": "", "ChromaID": "" });
                }
                let buddy = (w % 3 == 0).then(|| buddy_id(w % 5));
                json!({ "ID": weapon_id(w), "SkinID": skin_id(line, w), "SkinLevelID": level_id(line, w),
                        "ChromaID": chroma_id(line, w, usize::from(w == 12)), "CharmLevelID": buddy })
            })
            .collect();
        let lo = json!({
            "Guns": guns,
            "Sprays": (0..3).map(|n| json!({ "EquipSlotID": uuid(0xaa, n), "SprayID": spray_id(n) })).collect::<Vec<_>>(),
            "Identity": { "PlayerCardID": uuid(0xa8, 1), "PlayerTitleID": uuid(0xa9, 1), "AccountLevel": 187, "HideAccountLevel": false },
            "Incognito": false
        });
        let iso = |t: i64| {
            chrono::DateTime::from_timestamp(t, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_default()
        };
        let history: Vec<_> = (0..8)
            .rev()
            .map(|i| {
                let win = i % 3 != 1;
                let mut sources = vec![json!({ "ID": "time-played", "Amount": 3400 + i * 130 })];
                if win {
                    sources.push(json!({ "ID": "match-win", "Amount": 1000 }));
                }
                if i == 0 {
                    sources.push(json!({ "ID": "first-win-of-the-day", "Amount": 1000 }));
                }
                let total: i64 = sources.iter().filter_map(|s| s["Amount"].as_i64()).sum();
                json!({ "ID": format!("m{i}"), "MatchStart": iso(now - 3600 * (i + 1) * 5), "XPDelta": total, "XPSources": sources })
            })
            .collect();
        let xp = json!({ "Progress": { "Level": 187, "XP": 3150 }, "History": history,
                         "NextTimeFirstWinAvailable": iso(now + 14 * 3600 + 20 * 60) });
        let owned = Owned {
            skin_levels: Some(
                (0..WEAPONS.len())
                    .flat_map(|w| [level_id(w % 8, w), level_id((w + 3) % 8, w)])
                    .collect(),
            ),
            buddies: Some((0..5).map(buddy_id).collect()),
            sprays: Some((0..4).map(spray_id).collect()),
            cards: Some((0..63).map(|n| uuid(0xab, n)).collect()),
            titles: Some((0..41).map(|n| uuid(0xac, n)).collect()),
        };
        shop::build_loadout(
            &serde_json::from_value(lo).unwrap_or_default(),
            serde_json::from_value(xp).ok().as_ref(),
            &owned,
            &cosmetics(),
            now,
        )
    }
}
