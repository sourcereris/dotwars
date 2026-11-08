//! Dot Wars rules. Headless: no macroquad, no floats, no clock.
#![deny(clippy::disallowed_types, clippy::disallowed_methods)]

pub mod combat;
#[cfg(test)]
mod combat_tests;
pub mod components;
pub mod data;
pub mod economy;
pub mod production;
#[cfg(test)]
mod production_tests;
mod systems;

use components::{
    Body, Building, Combat, Construction, DropOff, GatherState, Health, LastHit, ObjectId, Order,
    Orders, PlayerStock, Position, PrevPosition, Producer, ResourceNode, Team, Worker,
};
use data::{BuildingKind, UnitKind};
use economy::{DROPOFF_RADIUS, NODE_AMOUNT, NODE_RADIUS, START_STOCK};
use hecs::{Entity, World};
use phalanx_core::{Checksum, Fixed, FixedVec2, IdGen, PlayerId, Rng};
use phalanx_sim::Simulation;
use phalanx_sim::grid::{Cell, Grid};
pub use production::{BuildError, TrainError};
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
    /// Workers harvest `node` and deliver to the nearest own drop-off. Other units ignore it.
    Gather {
        units: Vec<ObjectId>,
        node: ObjectId,
        queue: bool,
    },
    /// One worker walks to the site and places the building if it is still allowed on arrival.
    Build {
        worker: ObjectId,
        kind: BuildingKind,
        cell: Cell,
        queue: bool,
    },
    /// Workers resume an unfinished own building.
    Construct {
        units: Vec<ObjectId>,
        building: ObjectId,
        queue: bool,
    },
    /// Queue a unit in an own, finished building. Pays now.
    Train {
        building: ObjectId,
        kind: UnitKind,
    },
    /// Remove a queue item and refund it in full.
    CancelTrain {
        building: ObjectId,
        slot: usize,
    },
    SetRally {
        buildings: Vec<ObjectId>,
        target: FixedVec2,
    },
    /// Chase and hit `target`. Ignored if it is missing or not an enemy.
    Attack {
        units: Vec<ObjectId>,
        target: ObjectId,
        queue: bool,
    },
    /// Walk to `target`, fighting whatever comes within acquire range on the way.
    AttackMove {
        units: Vec<ObjectId>,
        target: FixedVec2,
        queue: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A worker delivered `amount` minerals to `dropoff`.
    Deposited {
        player: PlayerId,
        dropoff: ObjectId,
        amount: u32,
    },
    /// The node ran dry and is already gone, hence the position.
    NodeDepleted {
        node: ObjectId,
        pos: FixedVec2,
    },
    BuildFailed {
        player: PlayerId,
        reason: BuildError,
    },
    BuildingFinished {
        id: ObjectId,
    },
    TrainFailed {
        player: PlayerId,
        reason: TrainError,
    },
    UnitTrained {
        id: ObjectId,
        building: ObjectId,
    },
    /// A hit landed (instant in the MVP). `ranged`: the client draws a tracer instead of a flash.
    Attacked {
        attacker: ObjectId,
        target: ObjectId,
        ranged: bool,
    },
    /// Already removed from the world, hence the owner and position.
    Died {
        id: ObjectId,
        owner: PlayerId,
        pos: FixedVec2,
        building: bool,
    },
}

/// World units per grid cell, and the map in cells (2048 x 1536 units).
pub const CELL_SIZE: Fixed = Fixed::from_int(32);
pub const MAP_CELLS: (u16, u16) = (64, 48);

pub struct DotWars {
    pub world: World,
    pub rng: Rng,
    ids: IdGen,
    /// ObjectId -> Entity. Sorted by key, so iterating it is deterministic.
    index: BTreeMap<ObjectId, Entity>,
    /// Per-player stockpile. Not a component: it belongs to the player, not to an entity.
    stocks: BTreeMap<PlayerId, PlayerStock>,
    /// Who occupies each cell: buildings (whole footprint) and resource nodes (one cell).
    grid: Grid<Option<ObjectId>>,
    /// Ticks simulated so far. Only used to age `LastHit`.
    tick: u32,
}

impl DotWars {
    pub fn new(seed: u64) -> Self {
        Self {
            world: World::new(),
            rng: Rng::new(seed),
            ids: IdGen::default(),
            index: BTreeMap::new(),
            stocks: BTreeMap::new(),
            grid: Grid::new(MAP_CELLS.0, MAP_CELLS.1, CELL_SIZE, None),
            tick: 0,
        }
    }

    /// Deterministic start: one base per player at the left and right of the map, each with
    /// workers, a mineral field and two soldiers.
    pub fn demo_scenario(seed: u64) -> Self {
        let mut sim = Self::new(seed);
        sim.add_base(PlayerId(0), Cell::new(7, 14), 1);
        sim.add_base(PlayerId(1), Cell::new(55, 14), -1);
        sim
    }

    /// A finished Base at `cell`, a field of nodes on the side facing the middle (`dir` = 1 right,
    /// -1 left), five workers, two soldiers and the starting stock.
    fn add_base(&mut self, owner: PlayerId, cell: Cell, dir: i32) {
        let id = self.spawn_building(BuildingKind::Base, cell, owner, true);
        let c = self
            .entity(id)
            .and_then(|e| self.world.get::<&Position>(e).ok().map(|p| p.0));
        let (cx, cy) = c.map_or((0, 0), |p| (p.x.floor_int(), p.y.floor_int()));
        for (dx, dy) in [
            (170, -90),
            (190, -30),
            (190, 30),
            (170, 90),
            (140, 140),
            (140, -140),
        ] {
            self.spawn_node(FixedVec2::from_ints(cx + dx * dir, cy + dy), NODE_AMOUNT);
        }
        for i in 0..5 {
            self.spawn_worker(FixedVec2::from_ints(cx - 40 + i * 20, cy + 70), owner);
        }
        for i in 0..2 {
            self.spawn_unit(FixedVec2::from_ints(cx - 20 + i * 40, cy - 80), owner);
        }
        self.set_stock(owner, START_STOCK);
    }

    pub fn spawn_unit(&mut self, pos: FixedVec2, owner: PlayerId) -> ObjectId {
        self.spawn_kind(UnitKind::Soldier, pos, owner)
    }

    pub fn spawn_worker(&mut self, pos: FixedVec2, owner: PlayerId) -> ObjectId {
        self.spawn_kind(UnitKind::Worker, pos, owner)
    }

    /// Every unit comes from here: stats are read from `data`.
    pub fn spawn_kind(&mut self, kind: UnitKind, pos: FixedVec2, owner: PlayerId) -> ObjectId {
        let def = data::unit(kind);
        let id = ObjectId(self.ids.next_id());
        let e = self.world.spawn((
            id,
            Position(pos),
            PrevPosition(pos),
            Team(owner),
            Body { radius: def.radius },
            Orders::default(),
            kind,
            Health {
                hp: def.hp,
                max: def.hp,
            },
            Combat::default(),
        ));
        if kind == UnitKind::Worker {
            let _ = self.world.insert_one(e, Worker::default());
        }
        self.index.insert(id, e);
        id
    }

    pub fn spawn_node(&mut self, pos: FixedVec2, amount: u32) -> ObjectId {
        let id = ObjectId(self.ids.next_id());
        let e = self.world.spawn((
            id,
            Position(pos),
            Body {
                radius: NODE_RADIUS,
            },
            ResourceNode {
                amount,
                harvester: None,
            },
        ));
        self.index.insert(id, e);
        if let Some(c) = self.grid.cell_at(pos) {
            self.grid.set(c, Some(id));
        }
        id
    }

    pub fn spawn_dropoff(&mut self, pos: FixedVec2, owner: PlayerId) -> ObjectId {
        let id = ObjectId(self.ids.next_id());
        let e = self.world.spawn((
            id,
            Position(pos),
            Team(owner),
            Body {
                radius: DROPOFF_RADIUS,
            },
            DropOff,
        ));
        self.index.insert(id, e);
        id
    }

    /// Minerals in `player`'s stockpile.
    pub fn stock(&self, player: PlayerId) -> u32 {
        self.stocks.get(&player).map_or(0, |s| s.minerals)
    }

    pub fn set_stock(&mut self, player: PlayerId, minerals: u32) {
        self.stocks.entry(player).or_default().minerals = minerals;
    }

    pub fn despawn(&mut self, id: ObjectId) {
        if let Some(e) = self.index.remove(&id) {
            self.free_cells(id, e);
            let _ = self.world.despawn(e);
        }
    }

    /// Gives back the grid cells `id` occupies.
    fn free_cells(&mut self, id: ObjectId, e: Entity) {
        if let Ok(b) = self.world.get::<&Building>(e) {
            let (w, h) = data::building(b.kind).size;
            self.grid.fill_rect(b.cell, w, h, None);
        } else if self.world.satisfies::<&ResourceNode>(e).unwrap_or(false)
            && let Ok(p) = self.world.get::<&Position>(e)
            && let Some(c) = self.grid.cell_at(p.0)
            && self.grid.get(c) == Some(Some(id))
        {
            self.grid.set(c, None);
        }
    }

    pub fn grid(&self) -> &Grid<Option<ObjectId>> {
        &self.grid
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

    fn apply(&mut self, player: PlayerId, cmd: &Command, events: &mut Vec<Event>) {
        match cmd {
            Command::Move {
                units,
                target,
                queue,
            } => self.give_orders(player, units, Order::Move(*target), *queue),
            Command::Hold { units } => self.give_orders(player, units, Order::Hold, false),
            Command::Gather { units, node, queue } => {
                let is_node = self
                    .entity(*node)
                    .is_some_and(|e| self.world.satisfies::<&ResourceNode>(e).unwrap_or(false));
                if is_node {
                    let workers = self.only_workers(units);
                    self.give_orders(player, &workers, Order::Gather(*node), *queue);
                }
            }
            Command::Build {
                worker,
                kind,
                cell,
                queue,
            } => {
                let workers = self.only_workers(&[*worker]);
                let order = Order::Build {
                    kind: *kind,
                    cell: *cell,
                };
                self.give_orders(player, &workers, order, *queue);
            }
            Command::Construct {
                units,
                building,
                queue,
            } => {
                let unfinished = self.entity(*building).is_some_and(|e| {
                    self.world.get::<&Team>(e).is_ok_and(|t| t.0 == player)
                        && self.world.satisfies::<&Construction>(e).unwrap_or(false)
                });
                if unfinished {
                    let workers = self.only_workers(units);
                    self.give_orders(player, &workers, Order::Construct(*building), *queue);
                }
            }
            Command::Train { building, kind } => self.train(player, *building, *kind, events),
            Command::CancelTrain { building, slot } => self.cancel_train(player, *building, *slot),
            Command::SetRally { buildings, target } => self.set_rally(player, buildings, *target),
            Command::Attack {
                units,
                target,
                queue,
            } => {
                let enemy = self.entity(*target).is_some_and(|e| {
                    self.world.satisfies::<&Health>(e).unwrap_or(false)
                        && self.world.get::<&Team>(e).is_ok_and(|t| t.0 != player)
                });
                if enemy {
                    let fighters = self.only_fighters(units);
                    self.give_orders(player, &fighters, Order::Attack(*target), *queue);
                }
            }
            Command::AttackMove {
                units,
                target,
                queue,
            } => {
                let fighters = self.only_fighters(units);
                self.give_orders(player, &fighters, Order::AttackMove(*target), *queue);
            }
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
        Order::Gather(n) => {
            h.u32(3);
            h.u32(n.0);
        }
        Order::Build { kind, cell } => {
            h.u32(4);
            h.u32(*kind as u32);
            h.u32(cell.x as u32);
            h.u32(cell.y as u32);
        }
        Order::Construct(b) => {
            h.u32(5);
            h.u32(b.0);
        }
        Order::Attack(t) => {
            h.u32(6);
            h.u32(t.0);
        }
        Order::AttackMove(t) => {
            h.u32(7);
            h.i32(t.x.raw());
            h.i32(t.y.raw());
        }
    }
}

fn hash_gather_state(h: &mut Checksum, s: GatherState) {
    match s {
        GatherState::Idle => h.u32(0),
        GatherState::ToNode { node, waited } => {
            h.u32(1);
            h.u32(node.0);
            h.u32(waited as u32);
        }
        GatherState::Harvesting { node, timer } => {
            h.u32(2);
            h.u32(node.0);
            h.u32(timer.remaining() as u32);
        }
        GatherState::Returning { node } => {
            h.u32(3);
            h.u32(node.0);
        }
    }
}

impl Simulation for DotWars {
    type Command = Command;
    type Event = Event;

    fn tick(&mut self, commands: &[(PlayerId, Command)], events: &mut Vec<Event>) {
        // Fixed system order.
        systems::store_prev_positions(&mut self.world);
        for (player, cmd) in commands {
            self.apply(*player, cmd, events);
        }
        systems::advance_orders(&mut self.world);
        systems::movement(&mut self.world);
        self.run_gather(events);
        self.run_depletion(events);
        self.run_build(events);
        self.run_construct(events);
        self.run_produce(events);
        self.run_combat(events);
        self.run_death(events);
        self.tick += 1;
    }

    fn checksum(&self) -> u64 {
        let mut h = Checksum::default();
        let (s, i) = self.rng.state();
        h.u64(s);
        h.u64(i);
        h.u32(self.tick);
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
            if let Ok(w) = self.world.get::<&Worker>(e) {
                h.u32(w.carry);
                hash_gather_state(&mut h, w.state);
            }
            if let Ok(n) = self.world.get::<&ResourceNode>(e) {
                h.u32(n.amount);
                h.u32(n.harvester.map_or(u32::MAX, |i| i.0));
            }
            if self.world.satisfies::<&DropOff>(e).unwrap_or(false) {
                h.u32(0xD0);
            }
            if let Ok(k) = self.world.get::<&UnitKind>(e) {
                h.u32(*k as u32);
            }
            if let Ok(b) = self.world.get::<&Building>(e) {
                h.u32(b.kind as u32);
                h.u32(b.cell.x as u32);
                h.u32(b.cell.y as u32);
            }
            if let Ok(c) = self.world.get::<&Construction>(e) {
                h.u32(c.done as u32);
                h.u32(c.total as u32);
            }
            if let Ok(hp) = self.world.get::<&Health>(e) {
                h.u32(hp.hp);
                h.u32(hp.max);
            }
            if let Ok(c) = self.world.get::<&Combat>(e) {
                h.u32(c.cooldown.remaining() as u32);
                h.u32(c.target.map_or(u32::MAX, |t| t.0));
                match c.anchor {
                    Some(a) => {
                        h.i32(a.x.raw());
                        h.i32(a.y.raw());
                    }
                    None => h.u32(u32::MAX),
                }
                h.u32(c.retarget_in as u32);
            }
            if let Ok(l) = self.world.get::<&LastHit>(e) {
                h.u32(l.by.0);
                h.u32(l.tick);
            }
            if let Ok(p) = self.world.get::<&Producer>(e) {
                h.u32(p.queue.len() as u32);
                for k in &p.queue {
                    h.u32(*k as u32);
                }
                h.u32(p.progress as u32);
                match p.rally {
                    Some(r) => {
                        h.i32(r.x.raw());
                        h.i32(r.y.raw());
                    }
                    None => h.u32(u32::MAX),
                }
            }
        }
        for cell in self.grid.as_slice() {
            h.u32(cell.map_or(u32::MAX, |i| i.0));
        }
        for (p, s) in &self.stocks {
            h.u32(p.0 as u32);
            h.u32(s.minerals);
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
        // Units (with Orders): 2 soldiers + 5 workers each. Buildings are not units.
        let count = |p| {
            a.world
                .query::<(&Team, &Orders)>()
                .iter()
                .filter(|(_, (t, _))| t.0 == p)
                .count()
        };
        assert_eq!((count(P0), count(P1)), (7, 7));
        assert_eq!(a.sorted_units().len(), 14);
        assert_eq!(a.world.query::<&DropOff>().iter().count(), 2);
        assert_eq!(a.world.query::<&ResourceNode>().iter().count(), 12);
        assert_eq!((a.stock(P0), a.stock(P1)), (START_STOCK, START_STOCK));
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
