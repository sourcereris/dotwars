//! Dot Wars rules. Headless: no macroquad, no floats, no clock.
#![deny(clippy::disallowed_types, clippy::disallowed_methods)]

pub mod components;
mod systems;

use components::{Body, ObjectId, Order, Orders, Position, PrevPosition, Team};
use hecs::{Entity, World};
use phalanx_core::{Checksum, Fixed, FixedVec2, IdGen, PlayerId, Rng};
use phalanx_sim::Simulation;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Units are named by stable `ObjectId`, never by `hecs::Entity`.
    /// `queue: false` replaces the unit's orders, `true` appends.
    Move {
        units: Vec<ObjectId>,
        target: FixedVec2,
        queue: bool,
    },
    Stop {
        units: Vec<ObjectId>,
    },
    Hold {
        units: Vec<ObjectId>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {}

pub const UNIT_RADIUS: Fixed = Fixed::from_int(8);

pub struct DotWars {
    pub world: World,
    pub rng: Rng,
    ids: IdGen,
    /// ObjectId -> Entity. Sorted by key, so iterating it is deterministic.
    index: BTreeMap<ObjectId, Entity>,
}

impl DotWars {
    pub fn new(seed: u64) -> Self {
        Self {
            world: World::new(),
            rng: Rng::new(seed),
            ids: IdGen::default(),
            index: BTreeMap::new(),
        }
    }

    /// Deterministic start: a block of own units on the left, a smaller enemy block on the right.
    pub fn demo_scenario(seed: u64) -> Self {
        let mut sim = Self::new(seed);
        for row in 0..3 {
            for col in 0..4 {
                let pos = FixedVec2::from_ints(100 + col * 30, 200 + row * 30);
                sim.spawn_unit(pos, PlayerId(0));
            }
        }
        for row in 0..4 {
            for col in 0..2 {
                let pos = FixedVec2::from_ints(600 + col * 30, 180 + row * 30);
                sim.spawn_unit(pos, PlayerId(1));
            }
        }
        sim
    }

    pub fn spawn_unit(&mut self, pos: FixedVec2, owner: PlayerId) -> ObjectId {
        let id = ObjectId(self.ids.next_id());
        let e = self.world.spawn((
            id,
            Position(pos),
            PrevPosition(pos),
            Team(owner),
            Body {
                radius: UNIT_RADIUS,
            },
            Orders::default(),
        ));
        self.index.insert(id, e);
        id
    }

    pub fn despawn(&mut self, id: ObjectId) {
        if let Some(e) = self.index.remove(&id) {
            let _ = self.world.despawn(e);
        }
    }

    pub fn entity(&self, id: ObjectId) -> Option<Entity> {
        self.index.get(&id).copied()
    }

    /// All units sorted by `ObjectId`. Use whenever iteration order affects the outcome.
    pub fn sorted_units(&self) -> Vec<(ObjectId, Entity)> {
        self.sorted_objects()
            .into_iter()
            .filter(|(_, e)| self.world.satisfies::<&Orders>(*e).unwrap_or(false))
            .collect()
    }

    /// Everything the sim tracks (units, later buildings and resources), sorted by `ObjectId`.
    pub fn sorted_objects(&self) -> Vec<(ObjectId, Entity)> {
        self.index.iter().map(|(id, e)| (*id, *e)).collect()
    }

    /// Entities named by a command that exist and belong to `player`, in `ObjectId` order, without duplicates.
    fn owned_targets(&self, player: PlayerId, units: &[ObjectId]) -> Vec<Entity> {
        let unique: BTreeSet<ObjectId> = units.iter().copied().collect();
        unique
            .into_iter()
            .filter_map(|id| self.entity(id))
            .filter(|e| self.world.get::<&Team>(*e).is_ok_and(|t| t.0 == player))
            .collect()
    }

    fn give_orders(&mut self, player: PlayerId, units: &[ObjectId], order: Order, queue: bool) {
        for e in self.owned_targets(player, units) {
            if let Ok(mut o) = self.world.get::<&mut Orders>(e) {
                o.give(order, queue);
            }
        }
    }

    fn apply(&mut self, player: PlayerId, cmd: &Command) {
        match cmd {
            Command::Move {
                units,
                target,
                queue,
            } => self.give_orders(player, units, Order::Move(*target), *queue),
            Command::Hold { units } => self.give_orders(player, units, Order::Hold, false),
            Command::Stop { units } => {
                for e in self.owned_targets(player, units) {
                    if let Ok(mut o) = self.world.get::<&mut Orders>(e) {
                        o.clear();
                    }
                }
            }
        }
    }
}

fn hash_order(h: &mut Checksum, o: &Order) {
    match o {
        Order::Move(t) => {
            h.u32(1);
            h.i32(t.x.raw());
            h.i32(t.y.raw());
        }
        Order::Hold => h.u32(2),
    }
}

impl Simulation for DotWars {
    type Command = Command;
    type Event = Event;

    fn tick(&mut self, commands: &[(PlayerId, Command)], _events: &mut Vec<Event>) {
        // Fixed system order.
        systems::store_prev_positions(&mut self.world);
        for (player, cmd) in commands {
            self.apply(*player, cmd);
        }
        systems::advance_orders(&mut self.world);
        systems::movement(&mut self.world);
    }

    fn checksum(&self) -> u64 {
        let mut h = Checksum::default();
        let (s, i) = self.rng.state();
        h.u64(s);
        h.u64(i);
        for (id, e) in self.sorted_objects() {
            h.u32(id.0);
            if let Ok(p) = self.world.get::<&Position>(e) {
                h.i32(p.0.x.raw());
                h.i32(p.0.y.raw());
            }
            if let Ok(t) = self.world.get::<&Team>(e) {
                h.u32(t.0.0 as u32);
            }
            if let Ok(b) = self.world.get::<&Body>(e) {
                h.i32(b.radius.raw());
            }
            if let Ok(o) = self.world.get::<&Orders>(e) {
                match &o.current {
                    Some(c) => hash_order(&mut h, c),
                    None => h.u32(0),
                }
                h.u32(o.queue.len() as u32);
                for q in &o.queue {
                    hash_order(&mut h, q);
                }
            }
        }
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phalanx_sim::Driver;

    const P0: PlayerId = PlayerId(0);
    const P1: PlayerId = PlayerId(1);

    fn mv(units: Vec<ObjectId>, x: i32, y: i32, queue: bool) -> Command {
        Command::Move {
            units,
            target: FixedVec2::from_ints(x, y),
            queue,
        }
    }

    fn pos(d: &Driver<DotWars>, id: ObjectId) -> FixedVec2 {
        let e = d.sim.entity(id).unwrap();
        d.sim.world.get::<&Position>(e).unwrap().0
    }

    fn run_ticks(d: &mut Driver<DotWars>, n: usize) -> Vec<u64> {
        let mut ev = Vec::new();
        (0..n)
            .map(|_| {
                d.step(&mut ev);
                d.sim.checksum()
            })
            .collect()
    }

    fn single_unit_driver() -> (Driver<DotWars>, ObjectId) {
        let mut sim = DotWars::new(7);
        let a = sim.spawn_unit(FixedVec2::ZERO, P0);
        (Driver::new(sim, 3), a)
    }

    fn run(seed: u64) -> (Vec<u64>, FixedVec2) {
        let mut sim = DotWars::new(seed);
        let a = sim.spawn_unit(FixedVec2::ZERO, P0);
        let mut d = Driver::new(sim, 3);
        d.issue(P0, mv(vec![a], 100, 50, false));
        let sums = run_ticks(&mut d, 200);
        (sums, pos(&d, a))
    }

    #[test]
    fn determinism_same_commands_same_checksums() {
        assert_eq!(run(7).0, run(7).0);
    }

    #[test]
    fn unit_reaches_target() {
        assert_eq!(run(7).1, FixedVec2::from_ints(100, 50));
    }

    #[test]
    fn demo_scenario_is_deterministic_and_has_two_teams() {
        let a = DotWars::demo_scenario(1);
        let b = DotWars::demo_scenario(1);
        assert_eq!(a.checksum(), b.checksum());
        let count = |p| {
            a.world
                .query::<&Team>()
                .iter()
                .filter(|(_, t)| t.0 == p)
                .count()
        };
        assert_eq!((count(P0), count(P1)), (12, 8));
    }

    #[test]
    fn team_and_body_change_checksum() {
        let mut a = DotWars::new(1);
        let mut b = DotWars::new(1);
        let ia = a.spawn_unit(FixedVec2::ZERO, P0);
        b.spawn_unit(FixedVec2::ZERO, P1);
        assert_ne!(a.checksum(), b.checksum());

        let before = a.checksum();
        let e = a.entity(ia).unwrap();
        a.world.get::<&mut Body>(e).unwrap().radius = Fixed::from_int(9);
        assert_ne!(before, a.checksum());
    }

    #[test]
    fn ownership_is_enforced() {
        let (mut d, a) = single_unit_driver();
        d.issue(P1, mv(vec![a], 100, 0, false));
        let sums = run_ticks(&mut d, 30);

        let (mut clean, _) = single_unit_driver();
        let clean_sums = run_ticks(&mut clean, 30);
        assert_eq!(sums, clean_sums);
        assert_eq!(pos(&d, a), FixedVec2::ZERO);
    }

    #[test]
    fn stop_halts_a_moving_unit() {
        let (mut d, a) = single_unit_driver();
        d.issue(P0, mv(vec![a], 400, 0, false));
        run_ticks(&mut d, 10);
        d.issue(P0, Command::Stop { units: vec![a] });
        run_ticks(&mut d, 5); // stop lands after the input delay
        let stopped_at = pos(&d, a);
        assert!(stopped_at.x > Fixed::ZERO && stopped_at.x < Fixed::from_int(400));
        run_ticks(&mut d, 20);
        assert_eq!(pos(&d, a), stopped_at);
    }

    #[test]
    fn hold_halts_a_moving_unit() {
        let (mut d, a) = single_unit_driver();
        d.issue(P0, mv(vec![a], 400, 0, false));
        run_ticks(&mut d, 10);
        d.issue(P0, Command::Hold { units: vec![a] });
        run_ticks(&mut d, 5);
        let at = pos(&d, a);
        run_ticks(&mut d, 20);
        assert_eq!(pos(&d, a), at);
        let e = d.sim.entity(a).unwrap();
        assert_eq!(
            d.sim.world.get::<&Orders>(e).unwrap().current,
            Some(Order::Hold)
        );
    }

    #[test]
    fn queued_orders_run_in_sequence() {
        let (mut d, a) = single_unit_driver();
        d.issue(P0, mv(vec![a], 40, 0, false));
        d.issue(P0, mv(vec![a], 40, 40, true));
        // Command applies on the 4th tick, then 10 steps of 4 units to A; B starts one tick later.
        run_ticks(&mut d, 13);
        assert_eq!(pos(&d, a), FixedVec2::from_ints(40, 0));
        run_ticks(&mut d, 30);
        assert_eq!(pos(&d, a), FixedVec2::from_ints(40, 40));
    }

    #[test]
    fn dead_unit_in_command_is_skipped() {
        let mut sim = DotWars::new(7);
        let a = sim.spawn_unit(FixedVec2::ZERO, P0);
        let b = sim.spawn_unit(FixedVec2::ZERO, P0);
        sim.despawn(b);
        let mut d = Driver::new(sim, 3);
        d.issue(P0, mv(vec![b, a, ObjectId(999)], 40, 0, false));
        run_ticks(&mut d, 30);
        assert_eq!(pos(&d, a), FixedVec2::from_ints(40, 0));
    }

    #[test]
    fn unit_list_order_and_duplicates_do_not_matter() {
        let sums = |order: [usize; 5]| {
            let mut sim = DotWars::new(7);
            let u: Vec<_> = (0..3)
                .map(|i| sim.spawn_unit(FixedVec2::from_ints(i * 20, 0), P0))
                .collect();
            let mut d = Driver::new(sim, 3);
            let list = order.iter().map(|i| u[*i]).collect();
            d.issue(P0, mv(list, 100, 100, false));
            run_ticks(&mut d, 40)
        };
        assert_eq!(sums([0, 1, 2, 0, 1]), sums([2, 0, 1, 0, 2]));
    }
}
