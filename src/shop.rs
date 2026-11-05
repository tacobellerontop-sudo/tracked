//! What the Store and Loadout screens show: Riot's raw store / loadout
//! payloads joined with names and art from [`Cosmetics`].
//!
//! Pure functions, so they're testable without a client.

use std::collections::HashSet;

use crate::content::cosmetics::Cosmetics;
use crate::riot::store::{self, ids, AccountXp, FullLoadout, Storefront, Wallet};

/// XP needed per account level.
pub const XP_PER_LEVEL: u32 = 5000;

/// A rarity badge (Select / Deluxe / Premium / Exclusive / Ultra).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rarity {
    pub name: String,
    /// `RRGGBBAA` hex, may be empty.
    pub color: String,
    pub icon: String,
    pub rank: u32,
}

/// A price in one currency.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Price {
    pub currency: String,
    pub amount: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ItemKind {
    #[default]
    Skin,
    Buddy,
    Spray,
    Card,
    Title,
    Agent,
    Contract,
    Flex,
    Other,
}

impl ItemKind {
    pub fn from_type_id(id: &str) -> Self {
        match id.to_ascii_lowercase().as_str() {
            ids::SKIN_LEVEL | ids::SKIN_CHROMA => ItemKind::Skin,
            ids::BUDDY => ItemKind::Buddy,
            ids::SPRAY => ItemKind::Spray,
            ids::CARD => ItemKind::Card,
            ids::TITLE => ItemKind::Title,
            ids::AGENT => ItemKind::Agent,
            ids::CONTRACT => ItemKind::Contract,
            ids::FLEX => ItemKind::Flex,
            _ => ItemKind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Skin => "Skin",
            ItemKind::Buddy => "Gun buddy",
            ItemKind::Spray => "Spray",
            ItemKind::Card => "Player card",
            ItemKind::Title => "Title",
            ItemKind::Agent => "Agent",
            ItemKind::Contract => "Contract",
            ItemKind::Flex => "Flex",
            ItemKind::Other => "Item",
        }
    }
}

/// One thing for sale.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShopItem {
    pub item_id: String,
    pub kind: ItemKind,
    pub name: String,
    /// `https://` image, empty if unknown.
    pub icon: String,
    pub rarity: Option<Rarity>,
    /// What you'd pay now.
    pub price: Option<Price>,
    /// The undiscounted price, when there's a discount.
    pub base_price: Option<u32>,
    pub discount_pct: u32,
    pub quantity: u32,
    pub owned: bool,
    /// Night market: not flipped in the game yet.
    pub unseen: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BundleView {
    pub name: String,
    pub art: String,
    pub items: Vec<ShopItem>,
    pub price: Option<Price>,
    pub base_price: Option<u32>,
    /// Unix seconds.
    pub ends_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Balance {
    pub currency: String,
    pub name: String,
    pub icon: String,
    pub amount: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoreView {
    pub fetched_at: i64,
    /// VP, Radianite, Kingdom Credits (in that order), then anything else.
    pub wallet: Vec<Balance>,
    pub daily: Vec<ShopItem>,
    pub daily_ends_at: i64,
    pub night_market: Vec<ShopItem>,
    pub night_market_ends_at: i64,
    pub bundles: Vec<BundleView>,
    pub accessories: Vec<ShopItem>,
    pub accessories_ends_at: i64,
}

impl StoreView {
    pub fn currency_icon(&self, currency: &str) -> Option<&str> {
        self.wallet
            .iter()
            .find(|b| b.currency.eq_ignore_ascii_case(currency))
            .map(|b| b.icon.as_str())
            .filter(|i| !i.is_empty())
    }

    /// The soonest moment something in the store changes.
    pub fn next_reset(&self) -> Option<i64> {
        [self.daily_ends_at, self.night_market_ends_at, self.accessories_ends_at]
            .into_iter()
            .chain(self.bundles.iter().map(|b| b.ends_at))
            .filter(|t| *t > self.fetched_at)
            .min()
    }
}

/// Short label for a currency ("VP", "RP", "KC").
pub fn currency_short(id: &str) -> &'static str {
    match id.to_ascii_lowercase().as_str() {
        ids::VP => "VP",
        ids::RADIANITE => "RP",
        ids::KINGDOM_CREDITS => "KC",
        ids::FREE_AGENTS => "Agent tokens",
        _ => "",
    }
}

fn currency_rank(id: &str) -> usize {
    match id.to_ascii_lowercase().as_str() {
        ids::VP => 0,
        ids::RADIANITE => 1,
        ids::KINGDOM_CREDITS => 2,
        _ => 3,
    }
}

fn rarity(cos: &Cosmetics, item_id: &str) -> Option<Rarity> {
    let tier = cos.item(item_id)?.tier.clone()?;
    let t = cos.tier(&tier)?;
    Some(Rarity {
        name: t.name.clone(),
        color: t.color.clone(),
        icon: t.icon.clone(),
        rank: t.rank,
    })
}

/// Name, art and rarity for one item id.
fn describe(cos: &Cosmetics, type_id: &str, item_id: &str) -> ShopItem {
    let kind = ItemKind::from_type_id(type_id);
    let found = cos.item(item_id);
    ShopItem {
        item_id: item_id.to_ascii_lowercase(),
        kind,
        name: found
            .map(|i| i.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("Unknown {}", kind.label().to_lowercase())),
        icon: found.map(|i| i.icon.clone()).unwrap_or_default(),
        rarity: rarity(cos, item_id),
        quantity: 1,
        ..Default::default()
    }
}

/// The one currency an offer costs (VP first if it somehow has several).
fn price_of(cost: &std::collections::HashMap<String, u32>) -> Option<Price> {
    cost.iter().min_by_key(|(c, _)| currency_rank(c)).map(|(c, a)| Price {
        currency: c.to_ascii_lowercase(),
        amount: *a,
    })
}

/// Whole-percent discount from a base and a discounted price.
pub fn discount(base: u32, now: u32) -> u32 {
    if base == 0 || now >= base {
        0
    } else {
        (100.0 * (1.0 - now as f64 / base as f64)).round() as u32
    }
}

fn offer_item(cos: &Cosmetics, offer: &store::Offer, owned: &HashSet<String>) -> Option<ShopItem> {
    let r = offer.rewards.first()?;
    let mut it = describe(cos, &r.item_type_id, &r.item_id);
    it.quantity = r.quantity.max(1);
    it.price = price_of(&offer.cost);
    it.owned = is_owned(cos, owned, &it);
    Some(it)
}

fn is_owned(cos: &Cosmetics, owned: &HashSet<String>, it: &ShopItem) -> bool {
    owned.contains(&it.item_id) || (it.kind == ItemKind::Skin && owned.contains(&cos.skin_of(&it.item_id)))
}

/// Build the Store screen. `owned` holds lowercase ids of skin levels (and
/// anything else) you already own; `now` is unix seconds.
pub fn build_store(
    front: &Storefront,
    wallet: &Wallet,
    owned: &HashSet<String>,
    cos: &Cosmetics,
    now: i64,
) -> StoreView {
    let at = |secs: i64| if secs > 0 { now + secs } else { 0 };
    // Owned skins are tracked per level; also index by skin so a bundle's
    // level-1 entry matches.
    let owned: HashSet<String> = owned.iter().flat_map(|i| [i.clone(), cos.skin_of(i)]).collect();

    let mut wallet_rows: Vec<Balance> = wallet
        .balances
        .iter()
        .filter(|(c, _)| currency_rank(c) < 3)
        .map(|(c, a)| {
            let info = cos.currency(c);
            Balance {
                currency: c.to_ascii_lowercase(),
                name: info
                    .map(|i| i.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| currency_short(c).to_string()),
                icon: info.map(|i| i.icon.clone()).unwrap_or_default(),
                amount: *a,
            }
        })
        .collect();
    wallet_rows.sort_by_key(|b| currency_rank(&b.currency));

    let daily = front
        .skins_panel_layout
        .single_item_store_offers
        .iter()
        .filter_map(|o| offer_item(cos, o, &owned))
        .collect();

    let (night_market, night_market_ends_at) = match &front.bonus_store {
        Some(b) => (
            b.bonus_store_offers
                .iter()
                .filter_map(|o| {
                    let mut it = offer_item(cos, &o.offer, &owned)?;
                    let base = it.price.clone();
                    if let Some(now_price) = price_of(&o.discount_costs) {
                        it.price = Some(now_price);
                        it.base_price = base.map(|p| p.amount);
                    }
                    it.discount_pct = match (it.base_price, &it.price) {
                        (Some(b), Some(p)) => discount(b, p.amount),
                        _ => o.discount_percent.round().clamp(0.0, 100.0) as u32,
                    };
                    it.unseen = !o.is_seen;
                    Some(it)
                })
                .collect(),
            at(b.bonus_store_remaining_duration_in_seconds),
        ),
        None => (Vec::new(), 0),
    };

    let bundles = front
        .featured_bundle
        .bundles
        .iter()
        .map(|b| {
            let info = cos.bundle(&b.data_asset_id);
            let items = b
                .items
                .iter()
                .map(|bi| {
                    let mut it = describe(cos, &bi.item.item_type_id, &bi.item.item_id);
                    it.quantity = bi.item.amount.max(1);
                    let currency = if bi.currency_id.is_empty() {
                        b.currency_id.clone()
                    } else {
                        bi.currency_id.clone()
                    };
                    it.price = Some(Price {
                        currency: currency.to_ascii_lowercase(),
                        amount: bi.discounted_price,
                    });
                    if bi.discounted_price < bi.base_price {
                        it.base_price = Some(bi.base_price);
                    }
                    // Riot sends this as a fraction for bundles; the prices are
                    // what's certain, so derive it from them.
                    it.discount_pct = discount(bi.base_price, bi.discounted_price);
                    it.owned = is_owned(cos, &owned, &it);
                    it
                })
                .collect::<Vec<_>>();
            let price = b.total_discounted_cost.as_ref().and_then(price_of);
            let base = b.total_base_cost.as_ref().and_then(price_of).map(|p| p.amount);
            BundleView {
                name: info
                    .map(|i| i.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| "Featured bundle".into()),
                art: info.map(|i| i.art.clone()).unwrap_or_default(),
                base_price: base.filter(|bp| price.as_ref().is_some_and(|p| p.amount < *bp)),
                price,
                items,
                ends_at: at(b.duration_remaining_in_seconds),
            }
        })
        .collect();

    let (accessories, accessories_ends_at) = match &front.accessory_store {
        Some(a) => (
            a.accessory_store_offers
                .iter()
                .filter_map(|o| offer_item(cos, &o.offer, &owned))
                .collect(),
            at(a.accessory_store_remaining_duration_in_seconds),
        ),
        None => (Vec::new(), 0),
    };

    StoreView {
        fetched_at: now,
        wallet: wallet_rows,
        daily,
        daily_ends_at: at(front
            .skins_panel_layout
            .single_item_offers_remaining_duration_in_seconds),
        night_market,
        night_market_ends_at,
        bundles,
        accessories,
        accessories_ends_at,
    }
}

// ------------------------------------------------------------------ loadout

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GunView {
    pub weapon_id: String,
    pub weapon: String,
    pub skin: String,
    /// Set when a non-default chroma is equipped.
    pub variant: Option<String>,
    pub image: String,
    pub rarity: Option<Rarity>,
    /// (name, icon) of the attached gun buddy.
    pub buddy: Option<(String, String)>,
    /// Sort key (sidearms first, melee last).
    pub order: (u32, u32),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct XpGain {
    /// Unix seconds.
    pub at: i64,
    pub total: i32,
    /// (label, amount): "Match played", "Match win", "First win of the day"…
    pub sources: Vec<(String, i32)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct XpView {
    pub level: u32,
    pub xp: u32,
    /// Unix seconds the daily first-win bonus comes back; `None` = ready now.
    pub first_win_at: Option<i64>,
    /// Newest first.
    pub recent: Vec<XpGain>,
}

/// How much of each kind you own.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collection {
    pub skins: Option<usize>,
    pub buddies: Option<usize>,
    pub sprays: Option<usize>,
    pub cards: Option<usize>,
    pub titles: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadoutView {
    pub card_icon: String,
    pub card_wide: String,
    pub card_name: String,
    pub title: String,
    pub level: u32,
    pub level_hidden: bool,
    pub incognito: bool,
    pub guns: Vec<GunView>,
    /// (name, icon) for each equipped spray.
    pub sprays: Vec<(String, String)>,
    pub xp: Option<XpView>,
    pub collection: Collection,
}

/// Owned item lists from `/store/v1/entitlements`, `None` where a call failed.
#[derive(Debug, Clone, Default)]
pub struct Owned {
    pub skin_levels: Option<Vec<String>>,
    pub buddies: Option<Vec<String>>,
    pub sprays: Option<Vec<String>>,
    pub cards: Option<Vec<String>>,
    pub titles: Option<Vec<String>>,
}

fn distinct(v: &Option<Vec<String>>) -> Option<usize> {
    v.as_ref().map(|v| v.iter().collect::<HashSet<_>>().len())
}

pub fn xp_source_label(id: &str) -> String {
    match id {
        "time-played" => "Match played".into(),
        "match-win" => "Match win".into(),
        "first-win-of-the-day" => "First win of the day".into(),
        other => {
            let s = other.replace('-', " ");
            let mut c = s.chars();
            match c.next() {
                Some(f) => f.to_uppercase().chain(c).collect(),
                None => String::new(),
            }
        }
    }
}

fn parse_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.timestamp())
}

pub fn build_xp(xp: &AccountXp, now: i64) -> XpView {
    XpView {
        level: xp.progress.level,
        xp: xp.progress.xp,
        first_win_at: parse_time(&xp.next_time_first_win_available).filter(|t| *t > now),
        recent: xp
            .history
            .iter()
            .rev()
            .take(10)
            .map(|h| XpGain {
                at: parse_time(&h.match_start).unwrap_or(0),
                total: h.xp_delta,
                sources: h
                    .xp_sources
                    .iter()
                    .filter(|s| s.amount != 0)
                    .map(|s| (xp_source_label(&s.id), s.amount))
                    .collect(),
            })
            .collect(),
    }
}

pub fn build_loadout(
    lo: &FullLoadout,
    xp: Option<&AccountXp>,
    owned: &Owned,
    cos: &Cosmetics,
    now: i64,
) -> LoadoutView {
    let mut guns: Vec<GunView> = lo
        .guns
        .iter()
        .map(|g| {
            let weapon = cos.weapon(&g.id);
            let skin = cos.item(&g.skin_id);
            let chroma = cos.item(&g.chroma_id);
            let skin_name = skin
                .map(|s| s.name.clone())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| weapon.map(|w| w.name.clone()).unwrap_or_default());
            let variant = chroma
                .map(|c| c.name.clone())
                .filter(|n| !n.is_empty() && *n != skin_name);
            // A chroma's render shows the colour you actually equipped.
            let image = [chroma, cos.item(&g.skin_level_id), skin]
                .into_iter()
                .flatten()
                .map(|i| i.icon.clone())
                .find(|i| !i.is_empty())
                .or_else(|| weapon.map(|w| w.icon.clone()))
                .unwrap_or_default();
            let buddy = g
                .charm_level_id
                .as_deref()
                .or(g.charm_id.as_deref())
                .filter(|b| !b.is_empty())
                .map(|b| {
                    let i = cos.item(b);
                    (
                        i.map(|i| i.name.clone()).unwrap_or_else(|| "Gun buddy".into()),
                        i.map(|i| i.icon.clone()).unwrap_or_default(),
                    )
                });
            GunView {
                weapon_id: g.id.to_ascii_lowercase(),
                weapon: weapon.map(|w| w.name.clone()).unwrap_or_else(|| "Weapon".into()),
                skin: skin_name,
                variant,
                image,
                rarity: rarity(cos, &g.skin_id),
                buddy,
                order: weapon.map(|w| (w.order, w.cost)).unwrap_or((6, 0)),
            }
        })
        .collect();
    guns.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.weapon.cmp(&b.weapon)));

    let id = &lo.identity;
    let card = cos.item(&id.player_card_id);
    LoadoutView {
        card_icon: card.map(|c| c.icon.clone()).unwrap_or_default(),
        card_wide: card.map(|c| c.wide.clone()).unwrap_or_default(),
        card_name: card.map(|c| c.name.clone()).unwrap_or_default(),
        title: cos
            .item(&id.player_title_id)
            .map(|t| t.name.clone())
            .unwrap_or_default(),
        level: id.account_level,
        level_hidden: id.hide_account_level,
        incognito: lo.incognito,
        guns,
        sprays: lo
            .sprays
            .iter()
            .filter(|s| !s.spray_id.is_empty())
            .map(|s| {
                let i = cos.item(&s.spray_id);
                (
                    i.map(|i| i.name.clone()).unwrap_or_else(|| "Spray".into()),
                    i.map(|i| i.icon.clone()).unwrap_or_default(),
                )
            })
            .collect(),
        xp: xp.map(|x| build_xp(x, now)),
        collection: Collection {
            skins: owned
                .skin_levels
                .as_ref()
                .map(|v| v.iter().map(|l| cos.skin_of(l)).collect::<HashSet<_>>().len()),
            buddies: distinct(&owned.buddies),
            sprays: distinct(&owned.sprays),
            cards: distinct(&owned.cards),
            titles: distinct(&owned.titles),
        },
    }
}

/// "13h 05m", "4d 2h", "12m 30s".
pub fn fmt_countdown(secs: i64) -> String {
    let s = secs.max(0);
    let (d, h, m, sec) = (s / 86_400, s % 86_400 / 3600, s % 3600 / 60, s % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else {
        format!("{m}m {sec:02}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown() {
        assert_eq!(fmt_countdown(13 * 3600 + 5 * 60 + 9), "13h 05m");
        assert_eq!(fmt_countdown(4 * 86_400 + 2 * 3600), "4d 2h");
        assert_eq!(fmt_countdown(750), "12m 30s");
        assert_eq!(fmt_countdown(-5), "0m 00s");
    }

    #[test]
    fn xp_labels() {
        assert_eq!(xp_source_label("first-win-of-the-day"), "First win of the day");
        assert_eq!(xp_source_label("some-new-thing"), "Some new thing");
    }
}
