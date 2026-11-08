use phalanx_core::FixedVec2;

/// Stable, never-reused unit id: sort key and the way commands name units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position(pub FixedVec2);

/// Position at the start of the tick, for render interpolation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrevPosition(pub FixedVec2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveTarget(pub FixedVec2);
