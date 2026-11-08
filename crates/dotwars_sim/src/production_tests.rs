//! Headless tests for placement, construction, supply and production.

use crate::components::{
    Building, Construction, ObjectId, Order, Orders, Position, Producer, ResourceNode, Worker,
};
use crate::data::{self, BuildingKind, UnitKind};
use crate::production::{BuildError, TrainError};
use crate::{Command, DotWars, Event};
use phalanx_core::{FixedVec2, PlayerId};
use phalanx_sim::grid::Cell;
use phalanx_sim::{Driver, Simulation};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn at(x: i32, y: i32) -> FixedVec2 {
    FixedVec2::from_ints(x, y)
}

fn build(worker: ObjectId, kind: BuildingKind, x: u16, y: u16) -> Command {
    Command::Build {
        worker,
        kind,
        cell: Cell::new(x, y),
        queue: false,
    }
}

fn train(building: ObjectId, kind: UnitKind) -> Command {
    Command::Train { building, kind }
}

fn run(d: &mut Driver<DotWars>, n: usize, ev: &mut Vec<Event>) {
    for _ in 0..n {
        d.step(ev);
    }
}

/// Steps until `pred` holds (panics after `max` ticks); returns the ticks it took.
fn run_until(
    d: &mut Driver<DotWars>,
    ev: &mut Vec<Event>,
    max: usize,
    pred: impl Fn(&Driver<DotWars>, &[Event]) -> bool,
) -> usize {
    for i in 0..max {
        if pred(d, ev) {
            return i;
        }
        d.step(ev);
    }
    panic!("condition not reached in {max} ticks");
}

fn finished(ev: &[Event]) -> usize {
    ev.iter()
        .filter(|e| matches!(e, Event::BuildingFinished { .. }))
        .count()
}

fn trained(ev: &[Event]) -> usize {
    ev.iter()
        .filter(|e| matches!(e, Event::UnitTrained { .. }))
        .count()
}

fn world_with_worker() -> (DotWars, ObjectId) {
    let mut sim = DotWars::new(7);
    let w = sim.spawn_worker(at(200, 200), P0);
    sim.set_stock(P0, 1000);
    (sim, w)
}

fn construction(d: &Driver<DotWars>, id: ObjectId) -> Option<Construction> {
    let e = d.sim.entity(id)?;
    d.sim.world.get::<&Construction>(e).ok().map(|c| *c)
}

fn base_world() -> (DotWars, ObjectId) {
    let mut sim = DotWars::new(7);
    let base = sim.spawn_building(BuildingKind::Base, Cell::new(10, 10), P0, true);
    sim.set_stock(P0, 1000);
    (sim, base)
}

#[test]
fn can_place_reports_each_reason() {
    let (mut sim, _) = world_with_worker();
    sim.spawn_node(at(100, 100), 10); // cell (3, 3)
    sim.spawn_building(BuildingKind::Barracks, Cell::new(10, 10), P0, true);
    let c = |sim: &DotWars, x, y| sim.can_place(P0, BuildingKind::SupplyDepot, Cell::new(x, y));
    assert_eq!(c(&sim, 20, 20), Ok(()));
    assert_eq!(c(&sim, 63, 47), Err(BuildError::OutOfBounds));
    assert_eq!(c(&sim, 3, 3), Err(BuildError::Blocked)); // on minerals
    assert_eq!(c(&sim, 2, 2), Err(BuildError::Blocked)); // 2x2 touches the node cell
    assert_eq!(c(&sim, 11, 11), Err(BuildError::Blocked)); // on a building
    sim.set_stock(P0, 99);
    assert_eq!(c(&sim, 20, 20), Err(BuildError::NotEnoughMinerals));
}

#[test]
fn worker_walks_pays_places_and_finishes() {
    let (sim, w) = world_with_worker();
    let mut d = Driver::new(sim, 3);
    d.issue(P0, build(w, BuildingKind::SupplyDepot, 10, 6));
    let mut ev = Vec::new();
    let placed = run_until(&mut d, &mut ev, 200, |d, _| {
        d.sim.grid().get(Cell::new(10, 6)) == Some(Some(ObjectId(1)))
    });
    assert_eq!(d.sim.stock(P0), 900);
    assert!(placed > 3, "must walk first");
    for (x, y) in [(10, 6), (11, 6), (10, 7), (11, 7)] {
        assert_eq!(d.sim.grid().get(Cell::new(x, y)), Some(Some(ObjectId(1))));
    }
    // The builder touches the site when it places it, so progress starts on that very tick
    // (done = 1) and reaches the total of 400 on the 399th tick after.
    let t = run_until(&mut d, &mut ev, 600, |_, ev| finished(ev) == 1);
    assert_eq!(t, 399);
    assert_eq!(construction(&d, ObjectId(1)), None);
    assert_eq!(d.sim.supply(P0), (1, 8));
    let e = d.sim.entity(w).unwrap();
    assert_eq!(d.sim.world.get::<&Orders>(e).unwrap().current, None);
}

#[test]
fn failures_pay_nothing_and_say_why() {
    let (mut sim, w) = world_with_worker();
    sim.spawn_node(at(400, 200), 10); // cell (12, 6)
    let mut d = Driver::new(sim, 3);
    let mut ev = Vec::new();
    for (x, y) in [(63, 47), (12, 6)] {
        d.issue(P0, build(w, BuildingKind::Barracks, x, y));
        run(&mut d, 150, &mut ev);
    }
    assert_eq!(d.sim.stock(P0), 1000);
    assert_eq!(
        ev,
        vec![
            Event::BuildFailed {
                player: P0,
                reason: BuildError::OutOfBounds
            },
            Event::BuildFailed {
                player: P0,
                reason: BuildError::Blocked
            },
        ]
    );
}

#[test]
fn same_tick_race_lower_id_wins() {
    let mut sim = DotWars::new(7);
    let a = sim.spawn_worker(at(200, 200), P0);
    let b = sim.spawn_worker(at(200, 200), P0);
    sim.set_stock(P0, 1000);
    let mut d = Driver::new(sim, 3);
    d.issue(P0, build(b, BuildingKind::SupplyDepot, 10, 6));
    d.issue(P0, build(a, BuildingKind::SupplyDepot, 10, 6));
    let mut ev = Vec::new();
    run(&mut d, 100, &mut ev);
    assert_eq!(d.sim.stock(P0), 900, "only one depot is paid");
    let ea = d.sim.entity(a).unwrap();
    assert!(matches!(
        d.sim.world.get::<&Orders>(ea).unwrap().current,
        Some(Order::Construct(_))
    ));
    assert_eq!(
        ev,
        vec![Event::BuildFailed {
            player: P0,
            reason: BuildError::Blocked
        }]
    );
}

#[test]
fn money_spent_in_flight_fails_on_arrival() {
    let (sim, w) = world_with_worker();
    let mut d = Driver::new(sim, 3);
    d.issue(P0, build(w, BuildingKind::Barracks, 14, 6));
    let mut ev = Vec::new();
    run(&mut d, 8, &mut ev);
    d.sim.set_stock(P0, 10); // spent on something else meanwhile
    run(&mut d, 150, &mut ev);
    assert_eq!(
        ev,
        vec![Event::BuildFailed {
            player: P0,
            reason: BuildError::NotEnoughMinerals
        }]
    );
    assert_eq!(d.sim.grid().get(Cell::new(14, 6)), Some(None));
}

#[test]
fn leaving_pauses_and_another_worker_resumes() {
    let (mut sim, a) = world_with_worker();
    let b = sim.spawn_worker(at(250, 250), P0);
    let mut d = Driver::new(sim, 3);
    d.issue(P0, build(a, BuildingKind::SupplyDepot, 10, 6));
    let mut ev = Vec::new();
    let depot = ObjectId(2); // worker a = 0, b = 1
    run_until(&mut d, &mut ev, 300, |d, _| {
        construction(d, depot).is_some_and(|c| c.done >= 50)
    });
    d.issue(
        P0,
        Command::Move {
            units: vec![a],
            target: at(900, 900),
            queue: false,
        },
    );
    run(&mut d, 10, &mut ev);
    let paused = construction(&d, depot).unwrap().done;
    run(&mut d, 40, &mut ev);
    assert_eq!(construction(&d, depot).unwrap().done, paused);

    d.issue(
        P0,
        Command::Construct {
            units: vec![b],
            building: depot,
            queue: false,
        },
    );
    run_until(&mut d, &mut ev, 600, |_, ev| finished(ev) == 1);
    assert_eq!(construction(&d, depot), None);
}

#[test]
fn training_pays_spawns_and_walks_to_the_rally_point() {
    let (sim, base) = base_world();
    let mut d = Driver::new(sim, 3);
    d.issue(
        P0,
        Command::SetRally {
            buildings: vec![base],
            target: at(700, 400),
        },
    );
    d.issue(P0, train(base, UnitKind::Worker));
    let mut ev = Vec::new();
    run(&mut d, 4, &mut ev);
    assert_eq!(d.sim.stock(P0), 950);
    assert_eq!(d.sim.supply(P0), (1, 10)); // queued counts
    let t = run_until(&mut d, &mut ev, 400, |_, ev| trained(ev) == 1);
    assert!((235..=245).contains(&t), "trained after {t} ticks");
    let Some(Event::UnitTrained { id, building }) = ev
        .iter()
        .find(|e| matches!(e, Event::UnitTrained { .. }))
        .cloned()
    else {
        unreachable!()
    };
    assert_eq!(building, base);
    let e = d.sim.entity(id).unwrap();
    assert!(d.sim.world.satisfies::<&Worker>(e).unwrap());
    run(&mut d, 200, &mut ev);
    assert_eq!(d.sim.world.get::<&Position>(e).unwrap().0, at(700, 400));
}

#[test]
fn cancel_refunds_in_full_and_the_queue_is_limited() {
    let (sim, base) = base_world();
    let mut d = Driver::new(sim, 3);
    let mut ev = Vec::new();
    for _ in 0..6 {
        d.issue(P0, train(base, UnitKind::Worker));
    }
    run(&mut d, 4, &mut ev);
    assert_eq!(d.sim.stock(P0), 1000 - 5 * 50);
    assert_eq!(
        ev,
        vec![Event::TrainFailed {
            player: P0,
            reason: TrainError::QueueFull
        }]
    );
    run(&mut d, 100, &mut ev); // some progress on the front item
    d.issue(
        P0,
        Command::CancelTrain {
            building: base,
            slot: 0,
        },
    );
    d.issue(
        P0,
        Command::CancelTrain {
            building: base,
            slot: 9, // no such slot
        },
    );
    run(&mut d, 4, &mut ev);
    assert_eq!(d.sim.stock(P0), 1000 - 4 * 50);
    let e = d.sim.entity(base).unwrap();
    let p = d.sim.world.get::<&Producer>(e).unwrap();
    // Progress restarted for the next item (a few ticks may have run since the cancel).
    assert!(p.progress <= 4, "progress {}", p.progress);
    assert_eq!(p.queue.len(), 4);
}

#[test]
fn supply_blocks_training_until_a_depot_is_finished() {
    let (mut sim, base) = base_world();
    let barracks = sim.spawn_building(BuildingKind::Barracks, Cell::new(20, 10), P0, true);
    for i in 0..9 {
        sim.spawn_worker(at(100 + i * 20, 900), P0); // 9 of 10 supply used
    }
    let mut d = Driver::new(sim, 3);
    let mut ev = Vec::new();
    d.issue(P0, train(barracks, UnitKind::Soldier)); // needs 2, 1 free
    d.issue(P0, train(base, UnitKind::Worker)); // needs 1: fits
    d.issue(P0, train(base, UnitKind::Worker)); // the queued one counts: now full
    run(&mut d, 4, &mut ev);
    let blocked = Event::TrainFailed {
        player: P0,
        reason: TrainError::SupplyBlocked,
    };
    assert_eq!(ev, vec![blocked.clone(), blocked]);
    d.sim
        .spawn_building(BuildingKind::SupplyDepot, Cell::new(30, 10), P0, true);
    assert_eq!(d.sim.supply(P0), (10, 18));
    ev.clear();
    d.issue(P0, train(barracks, UnitKind::Soldier));
    run(&mut d, 4, &mut ev);
    assert!(ev.is_empty());
}

#[test]
fn unfinished_buildings_and_wrong_kinds_do_not_train() {
    let (mut sim, base) = base_world();
    let raw = sim.spawn_building(BuildingKind::Barracks, Cell::new(20, 10), P0, false);
    let mut d = Driver::new(sim, 3);
    let mut ev = Vec::new();
    d.issue(P0, train(raw, UnitKind::Soldier)); // still under construction
    d.issue(P0, train(base, UnitKind::Soldier)); // a base trains workers only
    run(&mut d, 4, &mut ev);
    assert_eq!(d.sim.stock(P0), 1000);
    assert!(ev.is_empty());
}

#[test]
fn enemies_cannot_use_my_buildings() {
    let make = |cmds: Vec<(PlayerId, Command)>| {
        let (mut sim, base) = base_world();
        let raw = sim.spawn_building(BuildingKind::Barracks, Cell::new(20, 10), P0, false);
        let w = sim.spawn_worker(at(900, 900), P1);
        sim.set_stock(P1, 1000);
        let mut d = Driver::new(sim, 3);
        for (p, c) in cmds {
            d.issue(p, c);
        }
        let sums: Vec<u64> = (0..60)
            .map(|_| {
                d.step(&mut Vec::new());
                d.sim.checksum()
            })
            .collect();
        (sums, base, raw, w)
    };
    let (clean, base, raw, w) = make(vec![]);
    let attack = vec![
        (P1, train(base, UnitKind::Worker)),
        (
            P1,
            Command::SetRally {
                buildings: vec![base],
                target: at(5, 5),
            },
        ),
        (
            P1,
            Command::CancelTrain {
                building: base,
                slot: 0,
            },
        ),
        (
            P1,
            Command::Construct {
                units: vec![w],
                building: raw,
                queue: false,
            },
        ),
    ];
    assert_eq!(make(attack).0, clean);
}

/// Rebuilds the occupancy grid from the objects and compares.
fn grid_matches_objects(sim: &DotWars) -> bool {
    let mut g = phalanx_sim::grid::Grid::new(
        crate::MAP_CELLS.0,
        crate::MAP_CELLS.1,
        crate::CELL_SIZE,
        None::<ObjectId>,
    );
    for (id, e) in sim.sorted_objects() {
        if let Ok(b) = sim.world.get::<&Building>(e) {
            let (w, h) = data::building(b.kind).size;
            g.fill_rect(b.cell, w, h, Some(id));
        } else if sim.world.satisfies::<&ResourceNode>(e).unwrap() {
            let p = sim.world.get::<&Position>(e).unwrap().0;
            if let Some(c) = g.cell_at(p) {
                g.set(c, Some(id));
            }
        }
    }
    &g == sim.grid()
}

#[test]
fn scripted_build_order_is_deterministic_and_keeps_the_grid_consistent() {
    let run_it = || {
        let mut sim = DotWars::new(3);
        sim.spawn_building(BuildingKind::Base, Cell::new(4, 4), P0, true); // id 0
        sim.spawn_node(at(400, 200), 100); // 1
        let w = sim.spawn_worker(at(300, 300), P0); // 2
        sim.set_stock(P0, 2000);
        let mut d = Driver::new(sim, 3);
        let mut ev = Vec::new();
        let mut sums = Vec::new();
        let mut step = |d: &mut Driver<DotWars>, ev: &mut Vec<Event>| {
            d.step(ev);
            sums.push(d.sim.checksum());
        };
        d.issue(P0, build(w, BuildingKind::SupplyDepot, 12, 4)); // 3
        while finished(&ev) < 1 {
            step(&mut d, &mut ev);
        }
        d.issue(P0, build(w, BuildingKind::Barracks, 16, 4)); // 4
        while finished(&ev) < 2 {
            step(&mut d, &mut ev);
        }
        for _ in 0..5 {
            d.issue(P0, train(ObjectId(4), UnitKind::Soldier));
        }
        while trained(&ev) < 5 {
            step(&mut d, &mut ev);
        }
        assert!(grid_matches_objects(&d.sim));
        assert_eq!(d.sim.supply(P0), (11, 18)); // worker + 5 soldiers; base 10 + depot 8
        sums
    };
    assert_eq!(run_it(), run_it());
}
