//! Unit and building stats as const tables. Placeholders; they move to `ron` files later.

use phalanx_core::Fixed;

/// Hard limit on supply, whatever the buildings add up to.
pub const SUPPLY_LIMIT: u32 = 100;
/// Items a production queue can hold.
pub const QUEUE_MAX: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnitKind {
    Worker,
    Soldier,
    Ranger,
    Brute,
}

/// Armor class. Bonus damage is keyed on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tag {
    Light,
    Medium,
    Heavy,
    Structure,
}

/// Attack range of a melee unit: touching plus a little.
pub const MELEE: Fixed = Fixed::from_int(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuildingKind {
    Base,
    Barracks,
    SupplyDepot,
}

pub struct UnitDef {
    pub name: &'static str,
    pub cost: u32,
    pub train_ticks: u16,
    pub supply: u32,
    pub radius: Fixed,
    pub hp: u32,
    pub damage: u32,
    /// Edge to edge.
    pub range: Fixed,
    /// Ticks between hits.
    pub cooldown: u16,
    pub speed: Fixed,
    /// Auto-acquire radius. `None`: never attacks on its own.
    pub acquire: Option<Fixed>,
    pub tag: Tag,
    /// Extra damage against one tag.
    pub bonus: Option<(Tag, u32)>,
}

pub struct BuildingDef {
    pub name: &'static str,
    pub cost: u32,
    pub build_ticks: u16,
    pub hp: u32,
    /// Footprint in grid cells.
    pub size: (u8, u8),
    /// Supply added once finished.
    pub supply: u32,
    pub trains: &'static [UnitKind],
    pub drop_off: bool,
}

const WORKER: UnitDef = UnitDef {
    name: "Worker",
    cost: 50,
    train_ticks: 240,
    supply: 1,
    radius: Fixed::from_int(6),
    hp: 40,
    damage: 4,
    range: MELEE,
    cooldown: 20,
    speed: Fixed::from_int(4),
    acquire: None,
    tag: Tag::Light,
    bonus: None,
};

const SOLDIER: UnitDef = UnitDef {
    name: "Soldier",
    cost: 75,
    train_ticks: 360,
    supply: 2,
    radius: Fixed::from_int(8),
    hp: 70,
    damage: 8,
    range: MELEE,
    cooldown: 16,
    speed: Fixed::from_int(4),
    acquire: Some(Fixed::from_int(160)),
    tag: Tag::Medium,
    bonus: Some((Tag::Light, 8)),
};

const RANGER: UnitDef = UnitDef {
    name: "Ranger",
    cost: 100,
    train_ticks: 400,
    supply: 2,
    radius: Fixed::from_int(7),
    hp: 45,
    damage: 8,
    range: Fixed::from_int(150),
    cooldown: 20,
    speed: Fixed::from_int(4),
    acquire: Some(Fixed::from_int(220)),
    tag: Tag::Light,
    bonus: Some((Tag::Heavy, 14)),
};

const BRUTE: UnitDef = UnitDef {
    name: "Brute",
    cost: 125,
    train_ticks: 480,
    supply: 3,
    radius: Fixed::from_int(11),
    hp: 130,
    damage: 16,
    range: MELEE,
    cooldown: 30,
    speed: Fixed::from_int(3),
    acquire: Some(Fixed::from_int(160)),
    tag: Tag::Heavy,
    bonus: Some((Tag::Medium, 8)),
};

const BASE: BuildingDef = BuildingDef {
    name: "Base",
    cost: 400,
    build_ticks: 1200,
    hp: 1000,
    size: (3, 3),
    supply: 10,
    trains: &[UnitKind::Worker],
    drop_off: true,
};

const BARRACKS: BuildingDef = BuildingDef {
    name: "Barracks",
    cost: 150,
    build_ticks: 600,
    hp: 600,
    size: (3, 3),
    supply: 0,
    trains: &[UnitKind::Soldier, UnitKind::Ranger, UnitKind::Brute],
    drop_off: false,
};

const SUPPLY_DEPOT: BuildingDef = BuildingDef {
    name: "Supply depot",
    cost: 100,
    build_ticks: 400,
    hp: 400,
    size: (2, 2),
    supply: 8,
    trains: &[],
    drop_off: false,
};

pub fn unit(k: UnitKind) -> &'static UnitDef {
    match k {
        UnitKind::Worker => &WORKER,
        UnitKind::Soldier => &SOLDIER,
        UnitKind::Ranger => &RANGER,
        UnitKind::Brute => &BRUTE,
    }
}

pub fn building(k: BuildingKind) -> &'static BuildingDef {
    match k {
        BuildingKind::Base => &BASE,
        BuildingKind::Barracks => &BARRACKS,
        BuildingKind::SupplyDepot => &SUPPLY_DEPOT,
    }
}
