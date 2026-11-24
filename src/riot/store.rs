//! Your own store, wallet, loadout, account XP and collection, from Riot's
//! player-data (PD) service with the tokens the local client issued.
//!
//! Every call here only *reads*: nothing buys, equips or changes anything.
//! (The storefront is a `POST` with an empty body, but it only returns the
//! store; that's how Riot's client asks for it.)

use std::collections::HashMap;

use serde::Deserialize;

use super::pd::LoadoutIdentity;
use super::RiotSession;
use crate::error::ApiError;

/// Currency and item-type ids Riot uses in store payloads.
pub mod ids {
    pub const VP: &str = "85ad13f7-3d1b-5128-9eb2-7cd8ee0b5741";
    pub const RADIANITE: &str = "e59aa87c-4cbf-517a-5983-6e81511be9b7";
    pub const KINGDOM_CREDITS: &str = "85ca954a-41f2-ce94-9b45-8ca3dd39a00d";
    pub const FREE_AGENTS: &str = "f08d4ae3-939c-4576-ab26-09ce1f23bb37";

    pub const SKIN_LEVEL: &str = "e7c63390-eda7-46e0-bb7a-a6abdacd2433";
    pub const SKIN_CHROMA: &str = "3ad1b2b2-acdb-4524-852f-954a76ddae0a";
    pub const AGENT: &str = "01bb38e1-da47-4e6a-9b3d-945fe4655707";
    pub const CONTRACT: &str = "f85cb6f7-33e5-4dc8-b609-ec7212301948";
    pub const BUDDY: &str = "dd3bf334-87f3-40bd-b043-682a57a8dc3a";
    pub const SPRAY: &str = "d5f120f8-ff8c-4aac-92ea-f2b5acbe9475";
    pub const CARD: &str = "3f296c07-64c3-494c-923b-fe692a4fa1bd";
    pub const TITLE: &str = "de7caa6b-adf7-4588-bbd1-143831e786c6";
    pub const FLEX: &str = "03a572de-4234-31ed-d344-ababa488f981";
}

// ---------------------------------------------------------------- storefront

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Storefront {
    pub featured_bundle: FeaturedBundle,
    pub skins_panel_layout: SkinsPanel,
    /// The night market; absent when it isn't running.
    pub bonus_store: Option<BonusStore>,
    /// Kingdom Credits store (weekly accessories).
    pub accessory_store: Option<AccessoryStore>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct FeaturedBundle {
    pub bundles: Vec<RawBundle>,
    pub bundle_remaining_duration_in_seconds: i64,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct RawBundle {
    #[serde(rename = "ID")]
    pub id: String,
    /// The bundle's uuid on valorant-api.com (name and art).
    #[serde(rename = "DataAssetID")]
    pub data_asset_id: String,
    #[serde(rename = "CurrencyID")]
    pub currency_id: String,
    pub items: Vec<BundleItem>,
    pub total_base_cost: Option<HashMap<String, u32>>,
    pub total_discounted_cost: Option<HashMap<String, u32>>,
    pub total_discount_percent: f32,
    pub duration_remaining_in_seconds: i64,
    pub wholesale_only: bool,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct BundleItem {
    pub item: BundleReward,
    pub base_price: u32,
    #[serde(rename = "CurrencyID")]
    pub currency_id: String,
    pub discount_percent: f32,
    pub discounted_price: u32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct BundleReward {
    #[serde(rename = "ItemTypeID")]
    pub item_type_id: String,
    #[serde(rename = "ItemID")]
    pub item_id: String,
    pub amount: u32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct SkinsPanel {
    pub single_item_store_offers: Vec<Offer>,
    pub single_item_offers_remaining_duration_in_seconds: i64,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Offer {
    #[serde(rename = "OfferID")]
    pub offer_id: String,
    /// currency id -> price.
    pub cost: HashMap<String, u32>,
    pub rewards: Vec<Reward>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Reward {
    #[serde(rename = "ItemTypeID")]
    pub item_type_id: String,
    #[serde(rename = "ItemID")]
    pub item_id: String,
    pub quantity: u32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct BonusStore {
    pub bonus_store_offers: Vec<BonusOffer>,
    pub bonus_store_remaining_duration_in_seconds: i64,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct BonusOffer {
    #[serde(rename = "BonusOfferID")]
    pub bonus_offer_id: String,
    pub offer: Offer,
    pub discount_percent: f32,
    pub discount_costs: HashMap<String, u32>,
    /// Whether you've flipped the card in the game yet.
    pub is_seen: bool,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct AccessoryStore {
    pub accessory_store_offers: Vec<AccessoryOffer>,
    pub accessory_store_remaining_duration_in_seconds: i64,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct AccessoryOffer {
    pub offer: Offer,
    #[serde(rename = "ContractID")]
    pub contract_id: String,
}

/// Your daily offers, featured bundles, night market and accessory store.
///
/// Riot's client asks with `POST /store/v3/storefront/{puuid}` and an empty
/// JSON body; older shards still answer `GET /store/v2/storefront/{puuid}`,
/// which is tried if v3 isn't there.
pub async fn storefront(s: &RiotSession, puuid: &str) -> Result<Storefront, ApiError> {
    check(puuid)?;
    let pd = s.pd()?;
    match s
        .post(&format!("{pd}/store/v3/storefront/{puuid}"), &serde_json::json!({}))
        .await
    {
        Err(ApiError::NotFound) | Err(ApiError::Status(405)) => {
            s.get(&format!("{pd}/store/v2/storefront/{puuid}")).await
        }
        other => other,
    }
}

// -------------------------------------------------------------------- wallet

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Wallet {
    /// currency id -> balance.
    pub balances: HashMap<String, i64>,
}

/// `GET /store/v1/wallet/{puuid}`: VP, Radianite, Kingdom Credits…
pub async fn wallet(s: &RiotSession, puuid: &str) -> Result<Wallet, ApiError> {
    check(puuid)?;
    s.get(&format!("{}/store/v1/wallet/{puuid}", s.pd()?)).await
}

// --------------------------------------------------------------- collection

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Entitlements {
    pub entitlements: Vec<Entitlement>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Entitlement {
    #[serde(rename = "ItemID")]
    pub item_id: String,
}

/// `GET /store/v1/entitlements/{puuid}/{itemTypeId}`: the items of one type
/// you own (skin levels, buddies, sprays, cards, titles…).
pub async fn owned(s: &RiotSession, puuid: &str, item_type: &str) -> Result<Vec<String>, ApiError> {
    check(puuid)?;
    if !super::pd::is_uuid(item_type) {
        return Err(ApiError::Other("invalid item type".into()));
    }
    let e: Entitlements = s
        .get(&format!("{}/store/v1/entitlements/{puuid}/{item_type}", s.pd()?))
        .await?;
    Ok(e.entitlements
        .into_iter()
        .map(|e| e.item_id.to_ascii_lowercase())
        .collect())
}

// ------------------------------------------------------------------- loadout

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct FullLoadout {
    pub guns: Vec<GunLoadout>,
    pub sprays: Vec<SprayLoadout>,
    pub identity: LoadoutIdentity,
    pub incognito: bool,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct GunLoadout {
    /// Weapon uuid.
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "SkinID")]
    pub skin_id: String,
    #[serde(rename = "SkinLevelID")]
    pub skin_level_id: String,
    #[serde(rename = "ChromaID")]
    pub chroma_id: String,
    /// Gun buddy (absent when none is attached).
    #[serde(rename = "CharmID")]
    pub charm_id: Option<String>,
    #[serde(rename = "CharmLevelID")]
    pub charm_level_id: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct SprayLoadout {
    #[serde(rename = "EquipSlotID")]
    pub equip_slot_id: String,
    #[serde(rename = "SprayID")]
    pub spray_id: String,
}

/// `GET /personalization/v2/players/{puuid}/playerloadout`: every equipped
/// skin, buddy, spray, card and title.
pub async fn loadout(s: &RiotSession, puuid: &str) -> Result<FullLoadout, ApiError> {
    check(puuid)?;
    s.get(&format!("{}/personalization/v2/players/{puuid}/playerloadout", s.pd()?))
        .await
}

// ---------------------------------------------------------------- account XP

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct AccountXp {
    pub progress: XpProgress,
    pub history: Vec<XpHistory>,
    /// RFC 3339; in the past when the daily first-win bonus is ready.
    pub next_time_first_win_available: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct XpProgress {
    pub level: u32,
    #[serde(rename = "XP")]
    pub xp: u32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct XpHistory {
    #[serde(rename = "ID")]
    pub id: String,
    pub match_start: String,
    #[serde(rename = "XPDelta")]
    pub xp_delta: i32,
    #[serde(rename = "XPSources")]
    pub xp_sources: Vec<XpSource>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct XpSource {
    #[serde(rename = "ID")]
    pub id: String,
    pub amount: i32,
}

/// `GET /account-xp/v1/players/{puuid}`: level, XP and recent XP gains.
pub async fn account_xp(s: &RiotSession, puuid: &str) -> Result<AccountXp, ApiError> {
    check(puuid)?;
    s.get(&format!("{}/account-xp/v1/players/{puuid}", s.pd()?)).await
}

fn check(puuid: &str) -> Result<(), ApiError> {
    if super::pd::is_uuid(puuid) {
        Ok(())
    } else {
        Err(ApiError::Other("invalid player id".into()))
    }
}
