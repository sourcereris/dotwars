use crate::data::{BuildingKind, UnitKind};
use phalanx_core::{Fixed, FixedVec2, PlayerId};
use phalanx_sim::{Cooldown, grid::Cell};

/// Stable, never-reused id for everything the sim tracks (units, buildings, resources):
/// sort key and the way commands name objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position(pub FixedVec2);

/// Position at the start of the tick, for render interpolation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrevPosition(pub FixedVec2);

/// Who owns the unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Team(pub PlayerId);

/// Unit size. Used for picking and drawing now, for separation later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Body {
    pub radius: Fixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Move(FixedVec2),
    /// Stand still. Differs from idle once combat exists (Hold never chases).
    Hold,
    /// Harvest the resource node and bring the cargo to the nearest own drop-off, repeatedly.
    Gather(ObjectId),
    /// Walk to the site, then place the building if it is still allowed and affordable.
    Build {
        kind: BuildingKind,
        cell: Cell,
    },
    /// Work on an unfinished own building until it is done.
    Construct(ObjectId),
    /// Chase and hit one object until it is gone.
    Attack(ObjectId),
    /// Walk to the point, fighting whatever comes within acquire range on the way.
    AttackMove(FixedVec2),
}

/// Current order plus a queue of following ones. `current == None` means idle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Orders {
    pub current: Option<Order>,
    pub queue: Vec<Order>,
}

impl Orders {
    /// `queue == false` replaces everything; `true` appends (or starts if idle).
    pub fn give(&mut self, order: Order, queue: bool) {
        if !queue {
            self.queue.clear();
            self.current = Some(order);
        } else if self.current.is_none() {
            self.current = Some(order);
        } else {
            self.queue.push(order);
        }
    }

    pub fn clear(&mut self) {
        self.current = None;
        self.queue.clear();
    }
}

/// A finite pile of minerals. `harvester` is the worker holding the single slot; a claim only counts
/// while that worker is still `Harvesting` this node (see `economy`), so it never needs cleanup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceNode {
    pub amount: u32,
    pub harvester: Option<ObjectId>,
}

/// Marker: workers deliver cargo here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropOff;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GatherState {
    #[default]
    Idle,
    /// `waited` counts ticks spent at an occupied node.
    ToNode {
        node: ObjectId,
        waited: u16,
    },
    Harvesting {
        node: ObjectId,
        timer: Cooldown,
    },
    /// Remembers the node to go back to.
    Returning {
        node: ObjectId,
    },
}

impl GatherState {
    pub fn node(self) -> Option<ObjectId> {
        match self {
            GatherState::Idle => None,
            GatherState::ToNode { node, .. }
            | GatherState::Harvesting { node, .. }
            | GatherState::Returning { node } => Some(node),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Worker {
    pub carry: u32,
    pub state: GatherState,
}

/// One player's stockpile. Lives next to the `World`, not on an entity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerStock {
    pub minerals: u32,
}

/// A building. `cell` is the top-left cell of its footprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Building {
    pub kind: BuildingKind,
    pub cell: Cell,
}

/// Present on a building until a worker has finished it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Construction {
    pub done: u16,
    pub total: u16,
}

/// Training queue of a finished building. Cost and supply are paid when an item is queued.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Producer {
    pub queue: Vec<UnitKind>,
    /// Ticks spent on the front item.
    pub progress: u16,
    /// Where new units walk to. `None`: they stay where they spawn.
    pub rally: Option<FixedVec2>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Health {
    pub hp: u32,
    pub max: u32,
}

/// Fighting state of a unit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Combat {
    pub cooldown: Cooldown,
    pub target: Option<ObjectId>,
    /// Where an idle unit was standing when it started a fight; it walks back there after.
    pub anchor: Option<FixedVec2>,
    /// Ticks until an auto-acquired target is re-evaluated.
    pub retarget_in: u8,
}

/// The last unit that hurt this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LastHit {
    pub by: ObjectId,
    pub tick: u32,
}
