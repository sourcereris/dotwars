use crate::components::{MoveTarget, Position, PrevPosition};
use hecs::World;
use phalanx_core::Fixed;

/// Placeholder speed until unit data files exist.
const SPEED: Fixed = Fixed::from_int(4);

pub fn store_prev_positions(world: &mut World) {
    for (_, (prev, pos)) in world.query_mut::<(&mut PrevPosition, &Position)>() {
        prev.0 = pos.0;
    }
}

/// Straight-line move. Each unit only touches itself, so query order does not matter.
pub fn movement(world: &mut World) {
    let mut arrived = Vec::new();
    for (e, (pos, target)) in world.query_mut::<(&mut Position, &MoveTarget)>() {
        let d = target.0 - pos.0;
        if d.length_sq() <= (SPEED * SPEED).raw() as i64 {
            pos.0 = target.0;
            arrived.push(e);
        } else {
            pos.0 = pos.0 + d.with_length(SPEED);
        }
    }
    for e in arrived {
        let _ = world.remove_one::<MoveTarget>(e);
    }
}
