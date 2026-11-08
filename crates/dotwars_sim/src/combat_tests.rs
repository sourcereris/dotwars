//! Headless tests for health, death and combat. Commands go straight into `tick` (no input delay).

use crate::combat::{LEASH, damage};
use crate::components::{Combat, Construction, Health, LastHit, ObjectId, Order, Orders, Position};
use crate::data::{self, BuildingKind, Tag, UnitKind};
use crate::{Command, DotWars, Event};
use phalanx_core::{FixedVec2, PlayerId};
use phalanx_sim::Simulation;
use phalanx_sim::grid::Cell;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn at(x: i32, y: i32) -> FixedVec2 {
    FixedVec2::from_ints(x, y)
}

/// One tick, returning only that tick's events.
fn tick(sim: &mut DotWars) -> Vec<Event> {
    let mut ev = Vec::new();
    sim.tick(&[], &mut ev);
    ev
}

fn tick_n(sim: &mut DotWars, n: usize) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..n {
        all.extend(tick(sim));
    }
    all
}

fn cmd(sim: &mut DotWars, player: PlayerId, c: Command) {
    let mut ev = Vec::new();
    sim.tick(&[(player, c)], &mut ev);
}

fn attack(units: &[ObjectId], target: ObjectId) -> Command {
    Command::Attack {
        units: units.to_vec(),
        target,
        queue: false,
    }
}

fn hp(sim: &DotWars, id: ObjectId) -> Option<u32> {
    let e = sim.entity(id)?;
    sim.world.get::<&Health>(e).ok().map(|h| h.hp)
}

fn pos(sim: &DotWars, id: ObjectId) -> FixedVec2 {
    let e = sim.entity(id).unwrap();
    sim.world.get::<&Position>(e).unwrap().0
}

fn target(sim: &DotWars, id: ObjectId) -> Option<ObjectId> {
    let e = sim.entity(id)?;
    sim.world.get::<&Combat>(e).ok()?.target
}

fn current(sim: &DotWars, id: ObjectId) -> Option<Order> {
    let e = sim.entity(id)?;
    sim.world.get::<&Orders>(e).ok()?.current
}

fn set_order(sim: &mut DotWars, id: ObjectId, o: Order) {
    let e = sim.entity(id).unwrap();
    sim.world.get::<&mut Orders>(e).unwrap().give(o, false);
}

fn died(ev: &[Event]) -> Vec<ObjectId> {
    ev.iter()
        .filter_map(|e| match e {
            Event::Died { id, .. } => Some(*id),
            _ => None,
        })
        .collect()
}

// ---------- K1: health and death ----------

#[test]
fn everything_spawns_with_full_health() {
    let mut sim = DotWars::new(1);
    for k in [
        UnitKind::Worker,
        UnitKind::Soldier,
        UnitKind::Ranger,
        UnitKind::Brute,
    ] {
        let id = sim.spawn_kind(k, at(10, 10), P0);
        assert_eq!(hp(&sim, id), Some(data::unit(k).hp));
    }
    let b = sim.spawn_building(BuildingKind::Barracks, Cell::new(10, 10), P0, true);
    assert_eq!(hp(&sim, b), Some(600));
}

#[test]
fn dead_objects_vanish_with_one_event_each_in_id_order() {
    let mut sim = DotWars::new(1);
    let a = sim.spawn_unit(at(0, 0), P0);
    let b = sim.spawn_unit(at(500, 0), P0);
    let c = sim.spawn_worker(at(1000, 0), P1);
    sim.hurt(a, 1000);
    sim.hurt(c, 1000);
    let ev = tick(&mut sim);
    assert_eq!(died(&ev), [a, c]);
    assert!(sim.entity(a).is_none() && sim.entity(c).is_none());
    assert!(sim.entity(b).is_some());
    let Some(Event::Died {
        owner,
        pos,
        building,
        ..
    }) = ev.last()
    else {
        panic!("no Died event");
    };
    assert_eq!((*owner, *pos, *building), (P1, at(1000, 0), false));
    assert!(tick(&mut sim).is_empty(), "no second event");
}

#[test]
fn dead_building_frees_its_cells_drops_the_queue_and_the_supply() {
    let mut sim = DotWars::new(1);
    sim.set_stock(P0, 1000);
    let base = sim.spawn_building(BuildingKind::Base, Cell::new(2, 2), P0, true);
    let barracks = sim.spawn_building(BuildingKind::Barracks, Cell::new(10, 10), P0, true);
    cmd(
        &mut sim,
        P0,
        Command::Train {
            building: barracks,
            kind: UnitKind::Soldier,
        },
    );
    assert_eq!(sim.supply(P0), (2, 10));
    let stock = sim.stock(P0);
    sim.hurt(barracks, 10_000);
    let ev = tick(&mut sim);
    assert!(matches!(
        ev.as_slice(),
        [Event::Died { building: true, .. }]
    ));
    assert_eq!(sim.grid().get(Cell::new(10, 10)), Some(None));
    assert_eq!(sim.grid().get(Cell::new(12, 12)), Some(None));
    assert_eq!(
        sim.can_place(P0, BuildingKind::Barracks, Cell::new(10, 10)),
        Ok(())
    );
    assert_eq!(sim.supply(P0), (0, 10));
    assert_eq!(sim.stock(P0), stock, "no refund");
    assert!(sim.entity(base).is_some());
}

#[test]
fn builder_goes_idle_when_the_site_dies() {
    let mut sim = DotWars::new(1);
    let site = sim.spawn_building(BuildingKind::Barracks, Cell::new(10, 10), P0, false);
    let w = sim.spawn_worker(at(368, 300), P0);
    cmd(
        &mut sim,
        P0,
        Command::Construct {
            units: vec![w],
            building: site,
            queue: false,
        },
    );
    tick_n(&mut sim, 5);
    sim.hurt(site, 10_000);
    let ev = tick_n(&mut sim, 3);
    assert_eq!(died(&ev), [site]);
    assert_eq!(current(&sim, w), None);
}

#[test]
fn health_is_part_of_the_checksum() {
    let mut a = DotWars::new(1);
    let mut b = DotWars::new(1);
    let ua = a.spawn_unit(at(0, 0), P0);
    b.spawn_unit(at(0, 0), P0);
    assert_eq!(a.checksum(), b.checksum());
    a.hurt(ua, 1);
    assert_ne!(a.checksum(), b.checksum());
}

// ---------- K2: attack ----------

#[test]
fn damage_uses_the_bonus_against_one_tag_only() {
    let s = data::unit(UnitKind::Soldier);
    assert_eq!(damage(s, Tag::Light), 16);
    assert_eq!(damage(s, Tag::Medium), 8);
    assert_eq!(damage(data::unit(UnitKind::Ranger), Tag::Heavy), 22);
    assert_eq!(damage(data::unit(UnitKind::Brute), Tag::Medium), 24);
    assert_eq!(damage(data::unit(UnitKind::Worker), Tag::Structure), 4);
}

#[test]
fn range_is_edge_to_edge_and_inclusive() {
    // Soldier (r 8, range 4) vs Worker (r 6): reach 18.
    for (dist, hit) in [(18, true), (19, false)] {
        let mut sim = DotWars::new(1);
        let s = sim.spawn_unit(at(0, 0), P0);
        let w = sim.spawn_worker(at(dist, 0), P1);
        set_order(&mut sim, s, Order::Hold);
        tick_n(&mut sim, 5);
        assert_eq!(hp(&sim, w) < Some(40), hit, "distance {dist}");
        assert_eq!(pos(&sim, s), at(0, 0), "Hold never moves");
    }
}

#[test]
fn hits_land_exactly_every_cooldown() {
    let mut sim = DotWars::new(1);
    let barracks = sim.spawn_building(BuildingKind::Barracks, Cell::new(10, 10), P1, true);
    // Barracks radius 48 + soldier 8 + melee 4.
    let s = sim.spawn_unit(at(368 - 60, 368), P0);
    cmd(&mut sim, P0, attack(&[s], barracks));
    let mut hit_ticks = Vec::new();
    let mut last = hp(&sim, barracks).unwrap();
    for t in 0..80 {
        tick(&mut sim);
        let now = hp(&sim, barracks).unwrap();
        if now < last {
            assert_eq!(last - now, 8);
            hit_ticks.push(t);
        }
        last = now;
    }
    assert!(hit_ticks.len() >= 4, "{hit_ticks:?}");
    assert!(
        hit_ticks.windows(2).all(|w| w[1] - w[0] == 16),
        "{hit_ticks:?}"
    );
}

#[test]
fn attack_chases_across_the_map_and_follows_a_moving_target() {
    let mut sim = DotWars::new(1);
    let s = sim.spawn_unit(at(0, 0), P0);
    let w = sim.spawn_worker(at(800, 0), P1);
    cmd(&mut sim, P0, attack(&[s], w));
    cmd(
        &mut sim,
        P1,
        Command::Move {
            units: vec![w],
            target: at(800, 600),
            queue: false,
        },
    );
    let ev = tick_n(&mut sim, 600);
    assert_eq!(died(&ev), [w]);
}

#[test]
fn equal_units_kill_each_other_on_the_same_tick_in_either_spawn_order() {
    for swap in [false, true] {
        let mut sim = DotWars::new(1);
        let (a, b) = if swap {
            let b = sim.spawn_unit(at(16, 0), P1);
            let a = sim.spawn_unit(at(0, 0), P0);
            (a, b)
        } else {
            let a = sim.spawn_unit(at(0, 0), P0);
            let b = sim.spawn_unit(at(16, 0), P1);
            (a, b)
        };
        sim.hurt(a, 62);
        sim.hurt(b, 62);
        let ev = tick_n(&mut sim, 3);
        let mut dead = died(&ev);
        dead.sort();
        let mut want = vec![a, b];
        want.sort();
        assert_eq!(dead, want, "swap = {swap}");
        // Both died in the very same tick: one batch of Died events.
        assert_eq!(
            ev.iter()
                .filter(|e| matches!(e, Event::Died { .. }))
                .count(),
            2
        );
    }
}

#[test]
fn attack_on_own_or_missing_objects_is_ignored() {
    let mut a = DotWars::new(1);
    let mut b = DotWars::new(1);
    for sim in [&mut a, &mut b] {
        sim.spawn_unit(at(0, 0), P0);
        sim.spawn_unit(at(200, 0), P0);
    }
    let mut ev = Vec::new();
    a.tick(&[(P0, attack(&[ObjectId(0)], ObjectId(1)))], &mut ev); // own
    a.tick(&[(P0, attack(&[ObjectId(0)], ObjectId(99)))], &mut ev); // missing
    b.tick(&[], &mut ev);
    b.tick(&[], &mut ev);
    assert_eq!(a.checksum(), b.checksum());
    assert_eq!(current(&a, ObjectId(0)), None);
}

#[test]
fn the_attacker_goes_idle_when_the_target_dies_and_takes_the_next_order() {
    let mut sim = DotWars::new(1);
    let s = sim.spawn_unit(at(0, 0), P0);
    let w = sim.spawn_worker(at(18, 0), P1);
    cmd(&mut sim, P0, attack(&[s], w));
    cmd(
        &mut sim,
        P0,
        Command::Move {
            units: vec![s],
            target: at(0, 300),
            queue: true,
        },
    );
    let ev = tick_n(&mut sim, 60);
    assert_eq!(died(&ev), [w]);
    assert!(pos(&sim, s).y > at(0, 0).y, "walked on to the queued move");
}

// ---------- K3: auto-acquire ----------

#[test]
fn armed_enemies_come_before_nearer_workers() {
    let mut sim = DotWars::new(1);
    let me = sim.spawn_unit(at(0, 0), P0);
    let _worker = sim.spawn_worker(at(40, 0), P1);
    let soldier = sim.spawn_unit(at(120, 0), P1);
    set_order(&mut sim, soldier, Order::Hold);
    tick(&mut sim);
    assert_eq!(target(&sim, me), Some(soldier));
}

#[test]
fn whoever_hit_me_recently_comes_first() {
    let mut sim = DotWars::new(1);
    let me = sim.spawn_unit(at(0, 0), P0);
    let near = sim.spawn_unit(at(40, 0), P1);
    let far = sim.spawn_unit(at(100, 0), P1);
    for id in [near, far] {
        set_order(&mut sim, id, Order::Hold);
    }
    let e = sim.entity(me).unwrap();
    sim.world
        .insert_one(e, LastHit { by: far, tick: 0 })
        .unwrap();
    tick(&mut sim);
    assert_eq!(target(&sim, me), Some(far));
}

#[test]
fn ties_resolve_by_id() {
    let mut sim = DotWars::new(1);
    let me = sim.spawn_unit(at(0, 0), P0);
    let first = sim.spawn_worker(at(50, 0), P1);
    let _second = sim.spawn_worker(at(50, 0), P1);
    tick(&mut sim);
    assert_eq!(target(&sim, me), Some(first));
}

#[test]
fn hold_fires_only_in_range_and_never_moves() {
    let mut sim = DotWars::new(1);
    let s = sim.spawn_unit(at(0, 0), P0);
    let w = sim.spawn_worker(at(100, 0), P1);
    set_order(&mut sim, s, Order::Hold);
    tick_n(&mut sim, 50);
    assert_eq!(pos(&sim, s), at(0, 0));
    assert_eq!(hp(&sim, w), Some(40));
}

#[test]
fn idle_units_chase_then_walk_back_to_their_anchor() {
    let mut sim = DotWars::new(1);
    let s = sim.spawn_unit(at(0, 0), P0);
    let w = sim.spawn_worker(at(150, 0), P1);
    let ev = tick_n(&mut sim, 200);
    assert_eq!(died(&ev), [w]);
    assert_eq!(pos(&sim, s), at(0, 0));
    let e = sim.entity(s).unwrap();
    assert_eq!(sim.world.get::<&Combat>(e).unwrap().anchor, None);
}

#[test]
fn the_leash_stops_a_chase_of_a_runaway() {
    let mut sim = DotWars::new(1);
    let s = sim.spawn_unit(at(0, 0), P0);
    let w = sim.spawn_worker(at(60, 0), P1);
    cmd(
        &mut sim,
        P1,
        Command::Move {
            units: vec![w],
            target: at(2000, 0),
            queue: false,
        },
    );
    let mut far = 0;
    for _ in 0..500 {
        tick(&mut sim);
        far = far.max(pos(&sim, s).x.floor_int());
    }
    assert!(far > 100, "it did chase ({far})");
    assert!(
        far <= LEASH.floor_int() + 8,
        "but not past the leash ({far})"
    );
    assert_eq!(pos(&sim, s), at(0, 0), "and it came back");
    assert_eq!(hp(&sim, w), Some(40));
}

#[test]
fn workers_never_pick_fights_on_their_own() {
    let mut sim = DotWars::new(1);
    let w = sim.spawn_worker(at(0, 0), P0);
    let e = sim.spawn_unit(at(20, 0), P1);
    set_order(&mut sim, e, Order::Hold);
    tick_n(&mut sim, 30);
    assert_eq!(target(&sim, w), None);
}

#[test]
fn units_that_are_moving_do_not_fight() {
    let mut sim = DotWars::new(1);
    let s = sim.spawn_unit(at(0, 0), P0);
    let w = sim.spawn_worker(at(60, 0), P1);
    cmd(
        &mut sim,
        P0,
        Command::Move {
            units: vec![s],
            target: at(300, 0),
            queue: false,
        },
    );
    tick_n(&mut sim, 20);
    assert_eq!(hp(&sim, w), Some(40));
    assert!(pos(&sim, s).x > at(60, 0).x, "walked straight through");
}

// ---------- K4: attack-move ----------

#[test]
fn attack_move_fights_on_the_way_and_then_arrives() {
    let mut sim = DotWars::new(1);
    let s = sim.spawn_unit(at(0, 0), P0);
    let w = sim.spawn_worker(at(150, 0), P1);
    cmd(
        &mut sim,
        P0,
        Command::AttackMove {
            units: vec![s],
            target: at(400, 0),
            queue: false,
        },
    );
    let ev = tick_n(&mut sim, 200);
    assert_eq!(died(&ev), [w]);
    assert_eq!(pos(&sim, s), at(400, 0));
    assert_eq!(current(&sim, s), None);
}

#[test]
fn attack_move_commands_skip_units_that_cannot_fight() {
    let mut sim = DotWars::new(1);
    let node = sim.spawn_node(at(500, 500), 100);
    cmd(
        &mut sim,
        P0,
        Command::AttackMove {
            units: vec![node],
            target: at(10, 10),
            queue: false,
        },
    );
    assert_eq!(current(&sim, node), None);
}

// ---------- K5: counters ----------

/// Two lines of `n` units, `gap` apart, all idle. Returns (survivors of A, survivors of B) after the fight.
fn brawl(a: UnitKind, b: UnitKind, n: i32) -> (usize, usize) {
    let gap = 140;
    let mut sim = DotWars::new(1);
    let mut left = Vec::new();
    let mut right = Vec::new();
    for i in 0..n {
        left.push(sim.spawn_kind(a, at(400, 400 + i * 24), P0));
        right.push(sim.spawn_kind(b, at(400 + gap, 400 + i * 24), P1));
    }
    tick_n(&mut sim, 3000);
    let alive = |ids: &[ObjectId]| ids.iter().filter(|i| sim.entity(**i).is_some()).count();
    (alive(&left), alive(&right))
}

#[test]
fn soldiers_beat_rangers() {
    let (s, r) = brawl(UnitKind::Soldier, UnitKind::Ranger, 5);
    assert!(s > 0 && r == 0, "soldiers {s}, rangers {r}");
}

#[test]
fn rangers_beat_brutes() {
    let (r, b) = brawl(UnitKind::Ranger, UnitKind::Brute, 5);
    assert!(r > 0 && b == 0, "rangers {r}, brutes {b}");
}

#[test]
fn brutes_beat_soldiers() {
    let (b, s) = brawl(UnitKind::Brute, UnitKind::Soldier, 5);
    assert!(b > 0 && s == 0, "brutes {b}, soldiers {s}");
}

#[test]
fn ranged_attacks_are_flagged_for_the_client() {
    let mut sim = DotWars::new(1);
    let r = sim.spawn_kind(UnitKind::Ranger, at(0, 0), P0);
    let w = sim.spawn_worker(at(100, 0), P1);
    let ev = tick_n(&mut sim, 3);
    assert!(ev.contains(&Event::Attacked {
        attacker: r,
        target: w,
        ranged: true
    }));
}

#[test]
fn a_brawl_is_deterministic() {
    let run = || {
        let mut sim = DotWars::new(5);
        for i in 0..20 {
            let (ka, kb) = match i % 3 {
                0 => (UnitKind::Soldier, UnitKind::Brute),
                1 => (UnitKind::Ranger, UnitKind::Soldier),
                _ => (UnitKind::Brute, UnitKind::Ranger),
            };
            sim.spawn_kind(ka, at(300 + (i % 4) * 20, 300 + i * 18), P0);
            sim.spawn_kind(kb, at(520 + (i % 4) * 20, 300 + i * 18), P1);
        }
        let mut sums = Vec::new();
        for _ in 0..600 {
            tick(&mut sim);
            sums.push(sim.checksum());
        }
        sums
    };
    assert_eq!(run(), run());
}

#[test]
fn barracks_trains_all_three_fighters() {
    let mut sim = DotWars::new(1);
    sim.set_stock(P0, 10_000);
    sim.spawn_building(BuildingKind::Base, Cell::new(2, 2), P0, true);
    let b = sim.spawn_building(BuildingKind::Barracks, Cell::new(10, 10), P0, true);
    for k in [UnitKind::Soldier, UnitKind::Ranger, UnitKind::Brute] {
        cmd(
            &mut sim,
            P0,
            Command::Train {
                building: b,
                kind: k,
            },
        );
    }
    assert_eq!(sim.supply(P0), (7, 10));
    let ev = tick_n(&mut sim, 1500);
    let n = ev
        .iter()
        .filter(|e| matches!(e, Event::UnitTrained { .. }))
        .count();
    assert_eq!(n, 3);
}

// ---------- K6: building hp ----------

#[test]
fn buildings_start_weak_and_grow_with_construction() {
    let mut sim = DotWars::new(1);
    let site = sim.spawn_building(BuildingKind::SupplyDepot, Cell::new(10, 10), P0, false);
    assert_eq!(hp(&sim, site), Some(40));
    let w = sim.spawn_worker(at(352, 300), P0);
    cmd(
        &mut sim,
        P0,
        Command::Construct {
            units: vec![w],
            building: site,
            queue: false,
        },
    );
    tick_n(&mut sim, 120);
    let mid = hp(&sim, site).unwrap();
    assert!(mid > 40 && mid < 400, "{mid}");
    tick_n(&mut sim, 400);
    let e = sim.entity(site).unwrap();
    assert!(!sim.world.satisfies::<&Construction>(e).unwrap());
    assert_eq!(hp(&sim, site), Some(400));
}

#[test]
fn damage_during_construction_is_kept() {
    let mut sim = DotWars::new(1);
    let site = sim.spawn_building(BuildingKind::SupplyDepot, Cell::new(10, 10), P0, false);
    sim.hurt(site, 10);
    let w = sim.spawn_worker(at(352, 300), P0);
    cmd(
        &mut sim,
        P0,
        Command::Construct {
            units: vec![w],
            building: site,
            queue: false,
        },
    );
    tick_n(&mut sim, 500);
    assert_eq!(hp(&sim, site), Some(390));
}

#[test]
fn an_unfinished_building_dies_and_frees_its_cells() {
    let mut sim = DotWars::new(1);
    let site = sim.spawn_building(BuildingKind::SupplyDepot, Cell::new(10, 10), P0, false);
    let s = sim.spawn_unit(at(352 - 40, 352), P1);
    cmd(&mut sim, P1, attack(&[s], site));
    let ev = tick_n(&mut sim, 16 * 6);
    assert_eq!(died(&ev), [site]);
    assert_eq!(sim.grid().get(Cell::new(10, 10)), Some(None));
}
