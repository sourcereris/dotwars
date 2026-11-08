use crate::components::{Order, Orders, Position, PrevPosition};
use hecs::World;
use phalanx_core::Fixed;

/// Placeholder speed until unit data files exist.
pub(crate) const SPEED: Fixed = Fixed::from_int(4);

pub fn store_prev_positions(world: &mut World) {
    for (_, (prev, pos)) in world.query_mut::<(&mut PrevPosition, &Position)>() {
        prev.0 = pos.0;
    }
}

/// Idle units take the next queued order. Each unit only touches itself, so query order is irrelevant.
pub fn advance_orders(world: &mut World) {
    for (_, orders) in world.query_mut::<&mut Orders>() {
        if orders.current.is_none() && !orders.queue.is_empty() {
            orders.current = Some(orders.queue.remove(0));
        }
    }
}

/// Straight-line move. Each unit only touches itself, so query order does not matter.
pub fn movement(world: &mut World) {
    for (_, (pos, orders)) in world.query_mut::<(&mut Position, &mut Orders)>() {
        let Some(Order::Move(target)) = orders.current else {
            continue;
        };
        let d = target - pos.0;
        if d.length_sq() <= (SPEED * SPEED).raw() as i64 {
            pos.0 = target;
            orders.current = None;
        } else {
            pos.0 = pos.0 + d.with_length(SPEED);
        }
    }
}
