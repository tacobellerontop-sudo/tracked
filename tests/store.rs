//! Store, wallet, loadout and account XP against the fake client (feature
//! `fake-riot`): the right endpoints, read-only, joined into what the
//! Store / Loadout screens show.

mod common;

use std::collections::HashSet;

use tracked::content::cosmetics::{ContentTier, Cosmetics, Item};
use tracked::riot::store::{self, ids};
use tracked::riot::{self};
use tracked::shop::{self, ItemKind, Owned};

fn cosmetics() -> Cosmetics {
    let mut c = Cosmetics::default();
    c.tiers.insert(
        "t-premium".into(),
        ContentTier {
            name: "Premium".into(),
            color: "d1548dff".into(),
            icon: String::new(),
            rank: 2,
        },
    );
    let skin = |name: &str, skin: &str| Item {
        name: name.into(),
        tier: Some("t-premium".into()),
        skin: Some(skin.into()),
        ..Default::default()
    };
    c.items
        .insert(common::SKIN_LEVEL_OWNED.into(), skin("Glass Vandal", "s1"));
    c.items.insert("s1".into(), skin("Glass Vandal", "s1"));
    c.items.insert("c1".into(), skin("Glass Vandal", "s1"));
    c.items
        .insert("6a1b2c3d-0000-4000-8000-000000000001".into(), skin("Ember Ghost", "s9"));
    c
}

#[tokio::test(flavor = "multi_thread")]
async fn store_and_loadout() {
    std::env::set_var("TRACKED_CLIENT_VERSION", common::CLIENT_VERSION);
    std::env::set_var("TRACKED_OFFLINE_CONTENT", "1");
    let port = common::start().await;
    std::env::set_var("TRACKED_LOCKFILE", common::write_lockfile(port, common::PASSWORD));
    std::env::set_var("TRACKED_FAKE_RIOT_BASE", format!("https://127.0.0.1:{port}"));
    let s = riot::connect().await.expect("connect");
    let p = s.puuid.clone();

    let front = store::storefront(&s, &p).await.expect("storefront (POST v3)");
    let wallet = store::wallet(&s, &p).await.expect("wallet");
    let owned = store::owned(&s, &p, ids::SKIN_LEVEL).await.expect("entitlements");
    assert_eq!(owned.len(), 2);

    let cos = cosmetics();
    let now = 1_800_000_000;
    let v = shop::build_store(
        &front,
        &wallet,
        &owned.iter().cloned().collect::<HashSet<_>>(),
        &cos,
        now,
    );

    // Wallet: VP, Radianite, Kingdom Credits, in that order.
    let amounts: Vec<i64> = v.wallet.iter().map(|b| b.amount).collect();
    assert_eq!(amounts, [1500, 40, 9000]);

    // Daily offers with names, rarity and prices; reset time is absolute.
    assert_eq!(v.daily.len(), 2);
    assert_eq!(v.daily[0].name, "Ember Ghost");
    assert_eq!(v.daily[0].rarity.as_ref().unwrap().name, "Premium");
    assert_eq!(v.daily[0].price.as_ref().unwrap().amount, 875);
    assert_eq!(v.daily[1].name, "Unknown skin");
    assert_eq!(v.daily_ends_at, now + 7200);

    // Night market: discounted price, base price, hidden-in-game flag.
    let nm = &v.night_market[0];
    assert_eq!(nm.price.as_ref().unwrap().amount, 1065);
    assert_eq!(nm.base_price, Some(1775));
    assert_eq!(nm.discount_pct, 40);
    assert!(nm.unseen);

    // Bundle: totals, per-item discount and the item we own.
    let b = &v.bundles[0];
    assert_eq!(b.price.as_ref().unwrap().amount, 2378);
    assert_eq!(b.base_price, Some(3550));
    assert_eq!(b.items[0].discount_pct, 33);
    assert!(b.items[0].owned);
    assert!(!b.items[1].owned);
    assert_eq!(v.next_reset(), Some(now + 3600));

    // Loadout + XP + collection.
    let lo = store::loadout(&s, &p).await.expect("loadout");
    let xp = store::account_xp(&s, &p).await.expect("account xp");
    let owned = Owned {
        skin_levels: Some(owned),
        buddies: store::owned(&s, &p, ids::BUDDY).await.ok(),
        ..Default::default()
    };
    let l = shop::build_loadout(&lo, Some(&xp), &owned, &cos, now);
    assert_eq!(l.level, 123);
    assert_eq!(l.guns.len(), 2);
    let vandal = l
        .guns
        .iter()
        .find(|g| g.weapon_id == "9c82e19d-4575-0200-1a81-3eacf00cf872")
        .unwrap();
    assert_eq!(vandal.skin, "Glass Vandal");
    assert!(vandal.variant.is_none(), "base chroma isn't a variant");
    assert!(vandal.buddy.is_some());
    assert_eq!(l.sprays.len(), 1);
    let x = l.xp.unwrap();
    assert_eq!((x.level, x.xp), (123, 2500));
    assert_eq!(x.first_win_at, None, "bonus time in the past means it's ready");
    assert_eq!(x.recent[0].total, 2500);
    assert_eq!(x.recent[0].sources[1], ("Match win".to_string(), 1000));
    // Two copies of one skin level count once; failed lookups stay unknown.
    assert_eq!(l.collection.skins, Some(1));
    assert_eq!(l.collection.buddies, Some(1));
    assert_eq!(l.collection.sprays, None);
}

#[test]
fn item_kinds() {
    assert_eq!(ItemKind::from_type_id(ids::SKIN_LEVEL), ItemKind::Skin);
    assert_eq!(ItemKind::from_type_id(&ids::BUDDY.to_uppercase()), ItemKind::Buddy);
    assert_eq!(ItemKind::from_type_id("nope"), ItemKind::Other);
}

#[test]
fn demo_store_and_loadout_are_complete() {
    let now = 1_800_000_000;
    let s = tracked::worker::shop::demo::store(now);
    assert_eq!(s.daily.len(), 4);
    assert_eq!(s.night_market.len(), 6);
    assert_eq!(s.bundles.len(), 2);
    assert_eq!(s.accessories.len(), 4);
    assert!(s.daily.iter().all(|i| !i.name.starts_with("Unknown")));
    assert!(s.bundles[0].items.iter().any(|i| i.owned));
    let l = tracked::worker::shop::demo::loadout(now);
    assert_eq!(l.guns.len(), 19);
    assert_eq!(l.guns.first().unwrap().weapon, "Classic");
    assert_eq!(l.guns.last().unwrap().weapon, "Melee");
    assert!(l.xp.unwrap().first_win_at.is_some());
}
