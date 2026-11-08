//! Gathering: the worker state machine, depletion and the deterministic "nearest" helper.
//!
//! Workers are stepped one by one in `ObjectId` order, because claiming a node touches another
//! entity. A claim (`ResourceNode::harvester`) only counts while that worker is still `Harvesting`
//! the node, so dead or reassigned workers never need cleanup.

use crate::components::{
    Body, DropOff, GatherState, ObjectId, Order, Orders, Position, ResourceNode, Team, Worker,
};
use crate::{DotWars, Event, systems::SPEED};
use hecs::Entity;
use phalanx_core::{Fixed, FixedVec2};
use phalanx_sim::{Cooldown, nearest_by};

// Placeholders until unit/building data files exist.
pub const NODE_AMOUNT: u32 = 1500;
pub const START_STOCK: u32 = 50;
/// Ticks spent harvesting one load (20 ticks = 1 s).
pub const HARVEST_TICKS: u16 = 20;
pub const CARRY: u32 = 5;
/// Ticks a worker waits at an occupied node before looking for a free one.
pub const WAIT_TICKS: u16 = 10;
/// Squared search radius (400 units) as `dist_sq` returns it: Q16.16 in an `i64`.
const SEARCH_RADIUS_SQ: i64 = (400 * 400) << Fixed::FRAC_BITS;

pub const NODE_RADIUS: Fixed = Fixed::from_int(12);
pub const DROPOFF_RADIUS: Fixed = Fixed::from_int(24);
pub const WORKER_RADIUS: Fixed = Fixed::from_int(6);

/// Moves up to `SPEED` toward `target` and stops once within `stop_at`. True when arrived.
pub(crate) fn step_toward(pos: &mut FixedVec2, target: FixedVec2, stop_at: Fixed) -> bool {
    let d = target - *pos;
    let len = d.length();
    if len <= stop_at {
        return true;
    }
    let remaining = len - stop_at;
    let step = if remaining < SPEED { remaining } else { SPEED };
    *pos = *pos + d.with_length(step);
    step == remaining
}

impl DotWars {
    /// Position and radius of a node that still has minerals.
    fn live_node(&self, id: ObjectId) -> Option<(FixedVec2, Fixed)> {
        let e = self.entity(id)?;
        let n = self.world.get::<&ResourceNode>(e).ok()?;
        if n.amount == 0 {
            return None;
        }
        let p = self.world.get::<&Position>(e).ok()?.0;
        let r = self.world.get::<&Body>(e).ok()?.radius;
        Some((p, r))
    }

    fn is_harvesting(&self, worker: ObjectId, node: ObjectId) -> bool {
        self.entity(worker)
            .and_then(|e| self.world.get::<&Worker>(e).ok().map(|w| w.state))
            .is_some_and(|s| matches!(s, GatherState::Harvesting { node: n, .. } if n == node))
    }

    /// Free if nobody holds it, `me` holds it, or the holder left (stale claim).
    fn slot_free(&self, node: ObjectId, me: ObjectId) -> bool {
        let Some(e) = self.entity(node) else {
            return false;
        };
        let Ok(n) = self.world.get::<&ResourceNode>(e) else {
            return false;
        };
        match n.harvester {
            None => true,
            Some(h) => h == me || !self.is_harvesting(h, node),
        }
    }

    /// Nearest node with minerals within the search radius.
    /// `require_free` skips occupied nodes; `exclude` skips one node.
    fn nearest_node(
        &self,
        from: FixedVec2,
        me: ObjectId,
        exclude: Option<ObjectId>,
        require_free: bool,
    ) -> Option<ObjectId> {
        let candidates = self.sorted_objects().into_iter().filter_map(|(id, e)| {
            if Some(id) == exclude {
                return None;
            }
            let n = self.world.get::<&ResourceNode>(e).ok()?;
            if n.amount == 0 {
                return None;
            }
            let p = self.world.get::<&Position>(e).ok()?.0;
            if from.dist_sq(p) > SEARCH_RADIUS_SQ {
                return None;
            }
            if require_free && !self.slot_free(id, me) {
                return None;
            }
            Some((id, p))
        });
        nearest_by(from, candidates)
    }

    /// Nearest drop-off of `team`: id, position, radius.
    fn nearest_dropoff(
        &self,
        from: FixedVec2,
        team: phalanx_core::PlayerId,
    ) -> Option<(ObjectId, FixedVec2, Fixed)> {
        let candidates = self.sorted_objects().into_iter().filter_map(|(id, e)| {
            if !self.world.satisfies::<&DropOff>(e).unwrap_or(false) {
                return None;
            }
            if self.world.get::<&Team>(e).ok()?.0 != team {
                return None;
            }
            Some((id, self.world.get::<&Position>(e).ok()?.0))
        });
        let id = nearest_by(from, candidates)?;
        let e = self.entity(id)?;
        let p = self.world.get::<&Position>(e).ok()?.0;
        let r = self.world.get::<&Body>(e).ok()?.radius;
        Some((id, p, r))
    }

    /// Steps every worker's gather state machine, in `ObjectId` order.
    pub(crate) fn run_gather(&mut self, events: &mut Vec<Event>) {
        let workers: Vec<(ObjectId, Entity)> = self
            .sorted_objects()
            .into_iter()
            .filter(|(_, e)| self.world.satisfies::<&Worker>(*e).unwrap_or(false))
            .collect();
        for (id, e) in workers {
            self.step_worker(id, e, events);
        }
    }

    fn step_worker(&mut self, id: ObjectId, e: Entity, events: &mut Vec<Event>) {
        let (Some(mut pos), Some(team), Some(wr), Some(cur), Some(mut w)) = (
            self.world.get::<&Position>(e).ok().map(|p| p.0),
            self.world.get::<&Team>(e).ok().map(|t| t.0),
            self.world.get::<&Body>(e).ok().map(|b| b.radius),
            self.world.get::<&Orders>(e).ok().map(|o| o.current),
            self.world.get::<&Worker>(e).ok().map(|w| *w),
        ) else {
            return;
        };
        // `Some(x)`: overwrite the current order with `x`.
        let mut new_order: Option<Option<Order>> = None;

        // The state follows the current order. A different order cancels the state
        // (cargo is kept, a claim just goes stale).
        let want = match cur {
            Some(Order::Gather(n)) => Some(n),
            _ => None,
        };
        if let Some(have) = w.state.node()
            && want != Some(have)
        {
            w.state = GatherState::Idle;
        }
        if w.state == GatherState::Idle
            && let Some(n) = want
        {
            w.state = if w.carry > 0 {
                GatherState::Returning { node: n }
            } else {
                GatherState::ToNode { node: n, waited: 0 }
            };
        }

        match w.state {
            GatherState::Idle => {}
            GatherState::ToNode { node, waited } => match self.live_node(node) {
                None => match self.nearest_node(pos, id, None, false) {
                    Some(n) => {
                        new_order = Some(Some(Order::Gather(n)));
                        w.state = GatherState::ToNode { node: n, waited: 0 };
                    }
                    None => {
                        new_order = Some(None);
                        w.state = GatherState::Idle;
                    }
                },
                Some((npos, nr)) => {
                    if !step_toward(&mut pos, npos, nr + wr) {
                        w.state = GatherState::ToNode { node, waited: 0 };
                    } else if self.slot_free(node, id) {
                        if let Some(ne) = self.entity(node)
                            && let Ok(mut n) = self.world.get::<&mut ResourceNode>(ne)
                        {
                            n.harvester = Some(id);
                        }
                        w.state = GatherState::Harvesting {
                            node,
                            timer: Cooldown::new(HARVEST_TICKS),
                        };
                    } else if waited + 1 >= WAIT_TICKS {
                        match self.nearest_node(pos, id, Some(node), true) {
                            Some(n) => {
                                new_order = Some(Some(Order::Gather(n)));
                                w.state = GatherState::ToNode { node: n, waited: 0 };
                            }
                            None => w.state = GatherState::ToNode { node, waited: 0 },
                        }
                    } else {
                        w.state = GatherState::ToNode {
                            node,
                            waited: waited + 1,
                        };
                    }
                }
            },
            GatherState::Harvesting { node, mut timer } => {
                if !timer.tick() {
                    w.state = GatherState::Harvesting { node, timer };
                } else {
                    if let Some(ne) = self.entity(node)
                        && let Ok(mut n) = self.world.get::<&mut ResourceNode>(ne)
                    {
                        let take = n.amount.min(CARRY);
                        n.amount -= take;
                        n.harvester = None;
                        w.carry += take;
                    }
                    w.state = GatherState::Returning { node };
                }
            }
            GatherState::Returning { node } => {
                // No drop-off: stand still and keep the cargo.
                if let Some((did, dpos, dr)) = self.nearest_dropoff(pos, team)
                    && step_toward(&mut pos, dpos, dr + wr)
                {
                    if w.carry > 0 {
                        self.stocks.entry(team).or_default().minerals += w.carry;
                        events.push(Event::Deposited {
                            player: team,
                            dropoff: did,
                            amount: w.carry,
                        });
                        w.carry = 0;
                    }
                    // If the node is gone, the next tick's `ToNode` retargets.
                    w.state = GatherState::ToNode { node, waited: 0 };
                }
            }
        }

        if let Ok(mut p) = self.world.get::<&mut Position>(e) {
            p.0 = pos;
        }
        if let Ok(mut ww) = self.world.get::<&mut Worker>(e) {
            *ww = w;
        }
        if let Some(o) = new_order
            && let Ok(mut orders) = self.world.get::<&mut Orders>(e)
        {
            orders.current = o;
        }
    }

    /// Removes empty nodes (lowest id first) and announces them.
    pub(crate) fn run_depletion(&mut self, events: &mut Vec<Event>) {
        let empty: Vec<(ObjectId, FixedVec2)> = self
            .sorted_objects()
            .into_iter()
            .filter_map(|(id, e)| {
                let n = self.world.get::<&ResourceNode>(e).ok()?;
                if n.amount != 0 {
                    return None;
                }
                Some((id, self.world.get::<&Position>(e).ok()?.0))
            })
            .collect();
        for (id, pos) in empty {
            events.push(Event::NodeDepleted { node: id, pos });
            self.despawn(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;
    use phalanx_core::PlayerId;
    use phalanx_sim::{Driver, Simulation};

    const P0: PlayerId = PlayerId(0);
    const P1: PlayerId = PlayerId(1);

    fn at(x: i32, y: i32) -> FixedVec2 {
        FixedVec2::from_ints(x, y)
    }

    fn gather(units: Vec<ObjectId>, node: ObjectId) -> Command {
        Command::Gather {
            units,
            node,
            queue: false,
        }
    }

    fn worker(d: &Driver<DotWars>, id: ObjectId) -> Worker {
        let e = d.sim.entity(id).unwrap();
        *d.sim.world.get::<&Worker>(e).unwrap()
    }

    fn amount(d: &Driver<DotWars>, node: ObjectId) -> Option<u32> {
        let e = d.sim.entity(node)?;
        Some(d.sim.world.get::<&ResourceNode>(e).unwrap().amount)
    }

    fn run(d: &mut Driver<DotWars>, n: usize, ev: &mut Vec<Event>) {
        for _ in 0..n {
            d.step(ev);
        }
    }

    /// Drop-off at the origin, one node 100 units to the right, one worker between them.
    fn line_world() -> (Driver<DotWars>, ObjectId, ObjectId) {
        let mut sim = DotWars::new(7);
        sim.spawn_dropoff(at(0, 0), P0);
        let node = sim.spawn_node(at(100, 0), NODE_AMOUNT);
        let w = sim.spawn_worker(at(50, 0), P0);
        sim.set_stock(P0, START_STOCK);
        (Driver::new(sim, 3), w, node)
    }

    #[test]
    fn one_trip_deposits_exactly_one_load() {
        let (mut d, w, node) = line_world();
        d.issue(P0, gather(vec![w], node));
        let mut ev = Vec::new();
        let mut ticks = 0;
        while d.sim.stock(P0) == START_STOCK {
            d.step(&mut ev);
            ticks += 1;
            assert!(ticks < 300, "worker never deposited");
        }
        // 3 ticks input delay + 8 walking + HARVEST_TICKS + 13 walking back, give or take rounding.
        assert!(ticks > 3 + HARVEST_TICKS as usize, "too fast: {ticks}");
        assert_eq!(d.sim.stock(P0), START_STOCK + CARRY);
        assert_eq!(amount(&d, node), Some(NODE_AMOUNT - CARRY));
        assert_eq!(worker(&d, w).carry, 0);
        assert_eq!(
            ev,
            vec![Event::Deposited {
                player: P0,
                dropoff: ObjectId(0),
                amount: CARRY
            }]
        );
    }

    #[test]
    fn trips_repeat() {
        let (mut d, w, node) = line_world();
        d.issue(P0, gather(vec![w], node));
        let mut ev = Vec::new();
        run(&mut d, 400, &mut ev);
        let trips = ev.len() as u32;
        assert!(trips >= 4, "only {trips} trips in 400 ticks");
        assert_eq!(d.sim.stock(P0), START_STOCK + trips * CARRY);
        // The load in the worker's hands is already off the node.
        let in_hand = worker(&d, w).carry;
        assert_eq!(
            amount(&d, node),
            Some(NODE_AMOUNT - trips * CARRY - in_hand)
        );
    }

    #[test]
    fn gather_rejects_enemy_non_worker_and_non_node() {
        // Ids follow spawn order: drop-off 0, node 1, worker 2, soldier 3.
        let (node, w, soldier) = (ObjectId(1), ObjectId(2), ObjectId(3));
        let build = |cmds: Vec<(PlayerId, Command)>| {
            let mut sim = DotWars::new(7);
            sim.spawn_dropoff(at(0, 0), P0);
            sim.spawn_node(at(100, 0), NODE_AMOUNT);
            sim.spawn_worker(at(50, 0), P0);
            sim.spawn_unit(at(60, 0), P0);
            let mut d = Driver::new(sim, 3);
            for (p, c) in cmds {
                d.issue(p, c);
            }
            (0..60)
                .map(|_| {
                    d.step(&mut Vec::new());
                    d.sim.checksum()
                })
                .collect::<Vec<_>>()
        };
        let baseline = build(vec![]);
        // Enemy tries to order our worker.
        assert_eq!(build(vec![(P1, gather(vec![w], node))]), baseline);
        // A plain unit ignores Gather.
        assert_eq!(build(vec![(P0, gather(vec![soldier], node))]), baseline);
        // The "node" is the drop-off, not a resource.
        assert_eq!(build(vec![(P0, gather(vec![w], ObjectId(0)))]), baseline);
    }

    #[test]
    fn stop_keeps_cargo_and_gather_returns_it_first() {
        let (mut d, w, node) = line_world();
        d.issue(P0, gather(vec![w], node));
        let mut ev = Vec::new();
        while !matches!(worker(&d, w).state, GatherState::Returning { .. }) {
            d.step(&mut ev);
        }
        assert_eq!(worker(&d, w).carry, CARRY);
        d.issue(P0, Command::Stop { units: vec![w] });
        run(&mut d, 6, &mut ev);
        assert_eq!(worker(&d, w).state, GatherState::Idle);
        assert_eq!(worker(&d, w).carry, CARRY);
        assert_eq!(d.sim.stock(P0), START_STOCK);

        d.issue(P0, gather(vec![w], node));
        while d.sim.stock(P0) == START_STOCK {
            d.step(&mut ev);
        }
        assert_eq!(d.sim.stock(P0), START_STOCK + CARRY);
    }

    #[test]
    fn depletion_splits_the_last_load_and_removes_the_node() {
        let mut sim = DotWars::new(7);
        sim.spawn_dropoff(at(0, 0), P0);
        let node = sim.spawn_node(at(100, 0), 7);
        let w = sim.spawn_worker(at(50, 0), P0);
        let mut d = Driver::new(sim, 3);
        d.issue(P0, gather(vec![w], node));
        let mut ev = Vec::new();
        run(&mut d, 300, &mut ev);

        let amounts: Vec<u32> = ev
            .iter()
            .filter_map(|e| match e {
                Event::Deposited { amount, .. } => Some(*amount),
                _ => None,
            })
            .collect();
        assert_eq!(amounts, [5, 2]);
        assert_eq!(d.sim.stock(P0), 7);
        let depleted: Vec<_> = ev
            .iter()
            .filter(|e| matches!(e, Event::NodeDepleted { .. }))
            .collect();
        assert_eq!(depleted.len(), 1);
        assert!(d.sim.entity(node).is_none());
        // Nothing else in range: idle, order cleared.
        assert_eq!(worker(&d, w).state, GatherState::Idle);
        let e = d.sim.entity(w).unwrap();
        assert_eq!(d.sim.world.get::<&Orders>(e).unwrap().current, None);
    }

    #[test]
    fn depleted_node_sends_workers_to_the_next_one() {
        let mut sim = DotWars::new(7);
        sim.spawn_dropoff(at(0, 0), P0);
        let a = sim.spawn_node(at(100, 0), 5);
        let b = sim.spawn_node(at(0, 150), NODE_AMOUNT);
        let w = sim.spawn_worker(at(50, 0), P0);
        let mut d = Driver::new(sim, 3);
        d.issue(P0, gather(vec![w], a));
        let mut ev = Vec::new();
        run(&mut d, 200, &mut ev);
        assert!(d.sim.entity(a).is_none());
        assert_eq!(worker(&d, w).state.node(), Some(b));
        assert!(d.sim.stock(P0) > 5);
    }

    #[test]
    fn node_out_of_search_radius_is_not_picked() {
        let mut sim = DotWars::new(7);
        sim.spawn_dropoff(at(0, 0), P0);
        let a = sim.spawn_node(at(100, 0), 5);
        sim.spawn_node(at(900, 0), NODE_AMOUNT); // too far
        let w = sim.spawn_worker(at(50, 0), P0);
        let mut d = Driver::new(sim, 3);
        d.issue(P0, gather(vec![w], a));
        run(&mut d, 200, &mut Vec::new());
        assert_eq!(worker(&d, w).state, GatherState::Idle);
    }

    #[test]
    fn same_tick_arrival_lower_id_wins_and_the_other_moves_on() {
        let build = || {
            let mut sim = DotWars::new(7);
            sim.spawn_dropoff(at(0, 0), P0);
            let n1 = sim.spawn_node(at(100, 0), NODE_AMOUNT);
            let n2 = sim.spawn_node(at(100, 60), NODE_AMOUNT);
            let a = sim.spawn_worker(at(60, 0), P0);
            let b = sim.spawn_worker(at(140, 0), P0);
            let mut d = Driver::new(sim, 3);
            d.issue(P0, gather(vec![a, b], n1));
            (d, a, b, n1, n2)
        };
        let (mut d, a, b, n1, n2) = build();
        let mut ev = Vec::new();
        run(&mut d, 24, &mut ev);
        assert!(matches!(
            worker(&d, a).state,
            GatherState::Harvesting { node, .. } if node == n1
        ));
        assert!(!matches!(
            worker(&d, b).state,
            GatherState::Harvesting { node, .. } if node == n1
        ));
        run(&mut d, 40, &mut ev);
        assert_eq!(worker(&d, b).state.node(), Some(n2));

        // Same result every run.
        let sums = |()| {
            let (mut d, ..) = build();
            (0..80)
                .map(|_| {
                    d.step(&mut Vec::new());
                    d.sim.checksum()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(sums(()), sums(()));
    }

    #[test]
    fn leaving_a_node_frees_its_slot() {
        let mut sim = DotWars::new(7);
        sim.spawn_dropoff(at(0, 0), P0);
        let n = sim.spawn_node(at(100, 0), NODE_AMOUNT);
        let a = sim.spawn_worker(at(60, 0), P0);
        let b = sim.spawn_worker(at(140, 0), P0);
        let mut d = Driver::new(sim, 3);
        d.issue(P0, gather(vec![a, b], n));
        let mut ev = Vec::new();
        run(&mut d, 20, &mut ev);
        assert!(matches!(
            worker(&d, a).state,
            GatherState::Harvesting { .. }
        ));
        d.issue(
            P0,
            Command::Move {
                units: vec![a],
                target: at(300, 300),
                queue: false,
            },
        );
        run(&mut d, 10, &mut ev);
        assert!(matches!(
            worker(&d, b).state,
            GatherState::Harvesting { node, .. } if node == n
        ));
    }

    #[test]
    fn six_workers_on_three_nodes_all_harvest() {
        let mut sim = DotWars::new(7);
        sim.spawn_dropoff(at(0, 0), P0);
        let nodes: Vec<_> = (0..3)
            .map(|i| sim.spawn_node(at(120, i * 50 - 50), NODE_AMOUNT))
            .collect();
        let ws: Vec<_> = (0..6)
            .map(|i| sim.spawn_worker(at(50, i * 10 - 25), P0))
            .collect();
        let mut d = Driver::new(sim, 3);
        d.issue(P0, gather(ws.clone(), nodes[0]));
        let mut ev = Vec::new();
        let mut harvested: std::collections::BTreeSet<ObjectId> = Default::default();
        for _ in 0..300 {
            d.step(&mut ev);
            for w in &ws {
                if matches!(worker(&d, *w).state, GatherState::Harvesting { .. }) {
                    harvested.insert(*w);
                }
            }
        }
        assert_eq!(harvested.len(), 6, "someone starved: {harvested:?}");
    }

    #[test]
    fn ten_workers_two_nodes_are_deterministic() {
        let run_it = || {
            let mut sim = DotWars::new(3);
            sim.spawn_dropoff(at(0, 0), P0);
            let n1 = sim.spawn_node(at(130, -40), NODE_AMOUNT);
            let n2 = sim.spawn_node(at(130, 40), 60);
            let ws: Vec<_> = (0..10)
                .map(|i| sim.spawn_worker(at(40, i * 8 - 40), P0))
                .collect();
            let mut d = Driver::new(sim, 3);
            d.issue(P0, gather(ws[..5].to_vec(), n1));
            d.issue(P0, gather(ws[5..].to_vec(), n2));
            let mut ev = Vec::new();
            let sums: Vec<u64> = (0..600)
                .map(|_| {
                    d.step(&mut ev);
                    d.sim.checksum()
                })
                .collect();
            (sums, d.sim.stock(P0))
        };
        let (a, stock) = run_it();
        assert_eq!(a, run_it().0);
        assert!(stock > START_STOCK);
    }

    #[test]
    fn stock_and_node_amount_change_the_checksum() {
        let (mut d, _, node) = line_world();
        let before = d.sim.checksum();
        d.sim.set_stock(P0, START_STOCK + 1);
        assert_ne!(before, d.sim.checksum());
        d.sim.set_stock(P0, START_STOCK);
        assert_eq!(before, d.sim.checksum());
        let e = d.sim.entity(node).unwrap();
        d.sim.world.get::<&mut ResourceNode>(e).unwrap().amount -= 1;
        assert_ne!(before, d.sim.checksum());
    }
}
