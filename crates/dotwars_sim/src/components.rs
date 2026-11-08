use phalanx_core::{Fixed, FixedVec2, PlayerId};

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
