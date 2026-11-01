//! Built-in weapon names and classes, so per-weapon stats read well even
//! before (or without) the valorant-api catalog. Ids are Riot's weapon UUIDs,
//! lowercase, as they appear in match details.

/// Pseudo-weapon id for kills made with abilities (ultimates, grenades…).
pub const ABILITIES: &str = "abilities";
/// The knife.
pub const MELEE: &str = "2f59173c-4bed-b6c3-2191-dea9b58be9c7";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    Rifle,
    Sniper,
    Smg,
    Shotgun,
    Heavy,
    Sidearm,
    Melee,
    Ability,
    Other,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Class::Rifle => "Rifle",
            Class::Sniper => "Sniper",
            Class::Smg => "SMG",
            Class::Shotgun => "Shotgun",
            Class::Heavy => "Heavy",
            Class::Sidearm => "Sidearm",
            Class::Melee => "Melee",
            Class::Ability => "Abilities",
            Class::Other => "Weapon",
        }
    }
}

const KNOWN: &[(&str, &str, Class)] = &[
    ("29a0cfab-485b-f5d5-779a-b59f85e204a8", "Classic", Class::Sidearm),
    ("42da8ccc-40d5-affc-beec-15aa47b42eda", "Shorty", Class::Sidearm),
    ("44d4e95c-4157-0037-81b2-17841bf2e8e3", "Frenzy", Class::Sidearm),
    ("1baa85b4-4c70-1284-64bb-6481dfc3bb4e", "Ghost", Class::Sidearm),
    ("e336c6b8-418d-9340-d77f-7a9e4cfe0702", "Sheriff", Class::Sidearm),
    ("f7e1b454-4ad4-1063-ec0a-159e56b58941", "Stinger", Class::Smg),
    ("462080d1-4035-2937-7c09-27aa2a5c27a7", "Spectre", Class::Smg),
    ("910be174-449b-c412-ab22-d0873436b21b", "Bucky", Class::Shotgun),
    ("ec845bf4-4f79-ddda-a3da-0db3774b2794", "Judge", Class::Shotgun),
    ("ae3de142-4d85-2547-dd26-4e90bed35cf7", "Bulldog", Class::Rifle),
    ("4ade7faa-4cf1-8376-95ef-39884480959b", "Guardian", Class::Rifle),
    ("ee8e8d15-496b-07ac-e5f6-8fae5d4c7b1a", "Phantom", Class::Rifle),
    ("9c82e19d-4575-0200-1a81-3eacf00cf872", "Vandal", Class::Rifle),
    ("c4883e50-4494-202c-3ec3-6b8a9284f00b", "Marshal", Class::Sniper),
    ("5f0aaf7a-4289-3998-d5ff-eb9a5cf7ef5c", "Outlaw", Class::Sniper),
    ("a03b24d3-4319-996d-0f8c-94bbfba1dfc7", "Operator", Class::Sniper),
    ("55d8a0f4-4274-ca67-fe2c-06ab45efdf58", "Ares", Class::Heavy),
    ("63e6c2b6-4a8e-869c-3d4c-e38355226584", "Odin", Class::Heavy),
    (MELEE, "Melee", Class::Melee),
];

/// Built-in name for a weapon id (any case), if we know it.
pub fn known_name(id: &str) -> Option<&'static str> {
    if id == ABILITIES {
        return Some("Abilities");
    }
    let id = id.to_ascii_lowercase();
    KNOWN.iter().find(|(k, ..)| *k == id).map(|(_, n, _)| *n)
}

pub fn class(id: &str) -> Class {
    if id == ABILITIES {
        return Class::Ability;
    }
    let id = id.to_ascii_lowercase();
    KNOWN
        .iter()
        .find(|(k, ..)| *k == id)
        .map(|(.., c)| *c)
        .unwrap_or(Class::Other)
}

/// Is this id one of the guns (not the knife, not abilities)?
pub fn is_gun(id: &str) -> bool {
    !matches!(class(id), Class::Melee | Class::Ability)
}

/// Is `name` a weapon we know by name ("vandal", any case)?
pub fn is_weapon_name(name: &str) -> bool {
    KNOWN.iter().any(|(_, n, _)| n.eq_ignore_ascii_case(name.trim()))
}
