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
}

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
}

pub struct BuildingDef {
    pub name: &'static str,
    pub cost: u32,
    pub build_ticks: u16,
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
};

const SOLDIER: UnitDef = UnitDef {
    name: "Soldier",
    cost: 75,
    train_ticks: 360,
    supply: 2,
    radius: Fixed::from_int(8),
};

const BASE: BuildingDef = BuildingDef {
    name: "Base",
    cost: 400,
    build_ticks: 1200,
    size: (3, 3),
    supply: 10,
    trains: &[UnitKind::Worker],
    drop_off: true,
};

const BARRACKS: BuildingDef = BuildingDef {
    name: "Barracks",
    cost: 150,
    build_ticks: 600,
    size: (3, 3),
    supply: 0,
    trains: &[UnitKind::Soldier],
    drop_off: false,
};

const SUPPLY_DEPOT: BuildingDef = BuildingDef {
    name: "Supply depot",
    cost: 100,
    build_ticks: 400,
    size: (2, 2),
    supply: 8,
    trains: &[],
    drop_off: false,
};

pub fn unit(k: UnitKind) -> &'static UnitDef {
    match k {
        UnitKind::Worker => &WORKER,
        UnitKind::Soldier => &SOLDIER,
    }
}

pub fn building(k: BuildingKind) -> &'static BuildingDef {
    match k {
        BuildingKind::Base => &BASE,
        BuildingKind::Barracks => &BARRACKS,
        BuildingKind::SupplyDepot => &SUPPLY_DEPOT,
    }
}
