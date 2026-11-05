//! Names and artwork for store and loadout items (skins, chromas, buddies,
//! sprays, cards, titles, bundles, currencies), from valorant-api.com.
//!
//! This is much bigger than the base [`super::Catalog`], so it's only
//! fetched the first time the Store or Loadout screen needs it.

use std::collections::HashMap;

use serde_json::Value;

use crate::error::ApiError;

const BASE: &str = "https://valorant-api.com";

/// One cosmetic item, keyed by its lowercase uuid in [`Cosmetics::items`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub name: String,
    /// Best available image (`https://`), empty if none.
    pub icon: String,
    /// Skin rarity (content tier uuid), for skins, levels and chromas.
    pub tier: Option<String>,
    /// For skin levels / chromas: the skin they belong to.
    pub skin: Option<String>,
    /// For skins, levels and chromas: the weapon uuid.
    pub weapon: Option<String>,
    /// Extra wide art (player cards).
    pub wide: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContentTier {
    /// "Premium", "Exclusive"…
    pub name: String,
    /// `RRGGBBAA` hex.
    pub color: String,
    pub icon: String,
    /// 0 (Select) … 4 (Ultra).
    pub rank: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeaponInfo {
    pub name: String,
    /// The default skin's image.
    pub icon: String,
    /// Sort key: sidearms first, melee last.
    pub order: u32,
    pub cost: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BundleInfo {
    pub name: String,
    /// Wide promo art.
    pub art: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CurrencyInfo {
    pub name: String,
    pub icon: String,
}

#[derive(Debug, Clone, Default)]
pub struct Cosmetics {
    pub items: HashMap<String, Item>,
    pub tiers: HashMap<String, ContentTier>,
    pub weapons: HashMap<String, WeaponInfo>,
    pub bundles: HashMap<String, BundleInfo>,
    pub currencies: HashMap<String, CurrencyInfo>,
}

impl Cosmetics {
    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.get(&id.to_ascii_lowercase())
    }

    pub fn tier(&self, id: &str) -> Option<&ContentTier> {
        self.tiers.get(&id.to_ascii_lowercase())
    }

    pub fn weapon(&self, id: &str) -> Option<&WeaponInfo> {
        self.weapons.get(&id.to_ascii_lowercase())
    }

    pub fn bundle(&self, id: &str) -> Option<&BundleInfo> {
        self.bundles.get(&id.to_ascii_lowercase())
    }

    pub fn currency(&self, id: &str) -> Option<&CurrencyInfo> {
        self.currencies.get(&id.to_ascii_lowercase())
    }

    /// The skin a skin level or chroma belongs to (or the id itself).
    pub fn skin_of(&self, id: &str) -> String {
        self.item(id)
            .and_then(|i| i.skin.clone())
            .unwrap_or_else(|| id.to_ascii_lowercase())
    }
}

async fn get(http: &reqwest::Client, path: &str) -> Result<Vec<Value>, ApiError> {
    let resp = http.get(format!("{BASE}{path}")).send().await?;
    if !resp.status().is_success() {
        return Err(ApiError::Status(resp.status().as_u16()));
    }
    let v: Value = resp.json().await?;
    match v.get("data") {
        Some(Value::Array(a)) => Ok(a.clone()),
        _ => Err(ApiError::Decode("valorant-api: no data array".into())),
    }
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn id(v: &Value) -> String {
    s(v, "uuid").to_ascii_lowercase()
}

/// First non-empty string among `keys`.
fn first(v: &Value, keys: &[&str]) -> String {
    keys.iter().map(|k| s(v, k)).find(|x| !x.is_empty()).unwrap_or_default()
}

fn arr<'a>(v: &'a Value, k: &str) -> impl Iterator<Item = &'a Value> {
    v.get(k).and_then(Value::as_array).into_iter().flatten()
}

/// Fetch everything. Sections that fail are left empty; it only errors if
/// nothing at all could be loaded.
pub async fn fetch(http: &reqwest::Client) -> Result<Cosmetics, ApiError> {
    let (weapons, tiers, buddies, sprays, cards, titles, bundles, currencies, flex) = tokio::join!(
        get(http, "/v1/weapons"),
        get(http, "/v1/contenttiers"),
        get(http, "/v1/buddies"),
        get(http, "/v1/sprays"),
        get(http, "/v1/playercards"),
        get(http, "/v1/playertitles"),
        get(http, "/v1/bundles"),
        get(http, "/v1/currencies"),
        get(http, "/v1/flex"),
    );
    if let (Err(e), Err(_)) = (&weapons, &bundles) {
        return Err(ApiError::Network(e.to_string()));
    }
    let mut c = Cosmetics::default();
    let v = |r: Result<Vec<Value>, ApiError>| r.unwrap_or_default();
    add_weapons(&mut c, &v(weapons));
    for t in v(tiers) {
        c.tiers.insert(
            id(&t),
            ContentTier {
                name: first(&t, &["devName", "displayName"]),
                color: s(&t, "highlightColor"),
                icon: s(&t, "displayIcon"),
                rank: t.get("rank").and_then(Value::as_u64).unwrap_or(0) as u32,
            },
        );
    }
    for b in v(buddies) {
        let name = s(&b, "displayName");
        let icon = s(&b, "displayIcon");
        for l in arr(&b, "levels") {
            c.items.insert(
                id(l),
                Item {
                    name: name.clone(),
                    icon: Some(s(l, "displayIcon"))
                        .filter(|x| !x.is_empty())
                        .unwrap_or_else(|| icon.clone()),
                    ..Default::default()
                },
            );
        }
        c.items.insert(
            id(&b),
            Item {
                name,
                icon,
                ..Default::default()
            },
        );
    }
    for sp in v(sprays) {
        let item = Item {
            name: s(&sp, "displayName"),
            icon: first(&sp, &["fullTransparentIcon", "displayIcon"]),
            ..Default::default()
        };
        for l in arr(&sp, "levels") {
            c.items.insert(id(l), item.clone());
        }
        c.items.insert(id(&sp), item);
    }
    for card in v(cards) {
        c.items.insert(
            id(&card),
            Item {
                name: s(&card, "displayName"),
                icon: first(&card, &["smallArt", "displayIcon"]),
                wide: s(&card, "wideArt"),
                ..Default::default()
            },
        );
    }
    for t in v(titles) {
        c.items.insert(
            id(&t),
            Item {
                name: first(&t, &["titleText", "displayName"]),
                ..Default::default()
            },
        );
    }
    for f in v(flex) {
        c.items.insert(
            id(&f),
            Item {
                name: s(&f, "displayName"),
                icon: s(&f, "displayIcon"),
                ..Default::default()
            },
        );
    }
    for b in v(bundles) {
        c.bundles.insert(
            id(&b),
            BundleInfo {
                name: s(&b, "displayName"),
                art: first(&b, &["displayIcon", "displayIcon2", "verticalPromoImage"]),
            },
        );
    }
    for cur in v(currencies) {
        c.currencies.insert(
            id(&cur),
            CurrencyInfo {
                name: s(&cur, "displayName"),
                icon: first(&cur, &["displayIcon", "largeIcon"]),
            },
        );
    }
    Ok(c)
}

/// Weapons, and every skin / level / chroma under them.
pub fn add_weapons(c: &mut Cosmetics, weapons: &[Value]) {
    for w in weapons {
        let weapon = id(w);
        let category = s(w, "category");
        let order = match category.rsplit("::").next().unwrap_or_default() {
            "Sidearm" => 0,
            "SMG" => 1,
            "Shotgun" => 2,
            "Rifle" => 3,
            "Sniper" => 4,
            "Heavy" => 5,
            "Melee" => 7,
            _ => 6,
        };
        let cost = w
            .get("shopData")
            .and_then(|d| d.get("cost"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        c.weapons.insert(
            weapon.clone(),
            WeaponInfo {
                name: s(w, "displayName"),
                icon: s(w, "displayIcon"),
                order,
                cost,
            },
        );
        for skin in arr(w, "skins") {
            let skin_id = id(skin);
            let name = s(skin, "displayName");
            let tier = Some(s(skin, "contentTierUuid").to_ascii_lowercase()).filter(|t| !t.is_empty());
            // The first chroma's render is the skin as it looks by default.
            let base_render = arr(skin, "chromas")
                .next()
                .map(|ch| first(ch, &["fullRender", "displayIcon"]))
                .unwrap_or_default();
            let skin_icon = Some(first(skin, &["displayIcon"]))
                .filter(|x| !x.is_empty())
                .unwrap_or_else(|| base_render.clone());
            let item = |name: String, icon: String| Item {
                name,
                icon,
                tier: tier.clone(),
                skin: Some(skin_id.clone()),
                weapon: Some(weapon.clone()),
                wide: String::new(),
            };
            for l in arr(skin, "levels") {
                let icon = Some(first(l, &["displayIcon"]))
                    .filter(|x| !x.is_empty())
                    .unwrap_or_else(|| skin_icon.clone());
                // Upgrades are named "Skin Level 2"; the store sells level 1,
                // which is just the skin's name.
                c.items.insert(id(l), item(name.clone(), icon));
            }
            for (i, ch) in arr(skin, "chromas").enumerate() {
                let icon = first(ch, &["fullRender", "displayIcon"]);
                let chroma_name = s(ch, "displayName");
                let label = if i == 0 { name.clone() } else { chroma_name };
                c.items.insert(id(ch), item(label, icon));
            }
            c.items.insert(skin_id.clone(), item(name.clone(), skin_icon));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skins_resolve_from_levels_and_chromas() {
        let weapons = serde_json::json!([{
            "uuid": "W-1", "displayName": "Vandal", "category": "EEquippableCategory::Rifle",
            "displayIcon": "https://x/vandal.png", "shopData": { "cost": 2900 },
            "skins": [{
                "uuid": "S-1", "displayName": "Prime Vandal", "contentTierUuid": "T-1", "displayIcon": null,
                "chromas": [
                    { "uuid": "C-1", "displayName": "Prime Vandal", "fullRender": "https://x/c1.png" },
                    { "uuid": "C-2", "displayName": "Prime Vandal Variant 1 Gold", "fullRender": "https://x/c2.png" }
                ],
                "levels": [{ "uuid": "L-1", "displayName": "Prime Vandal", "displayIcon": null }]
            }]
        }]);
        let mut c = Cosmetics::default();
        add_weapons(&mut c, weapons.as_array().unwrap());
        let l = c.item("L-1").unwrap();
        assert_eq!(l.name, "Prime Vandal");
        assert_eq!(l.icon, "https://x/c1.png");
        assert_eq!(l.tier.as_deref(), Some("t-1"));
        assert_eq!(c.skin_of("L-1"), "s-1");
        assert_eq!(c.item("c-2").unwrap().name, "Prime Vandal Variant 1 Gold");
        assert_eq!(c.weapon("w-1").unwrap().order, 3);
    }
}
