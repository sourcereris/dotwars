//! Placement, construction, supply and unit production.
//!
//! `can_place` is one pure function: the client calls it for the ghost, the sim calls it again when the
//! worker arrives. Anything that spawns entities walks its candidates in `ObjectId` order, because the
//! new ids must come out the same on every machine.

use crate::components::{
    Body, Building, Construction, DropOff, Health, ObjectId, Order, Orders, Position, Producer,
    Team, Worker,
};
use crate::data::{self, BuildingKind, QUEUE_MAX, SUPPLY_LIMIT, UnitKind};
use crate::economy::{WORKER_SPEED, step_toward};
use crate::{DotWars, Event};
use hecs::Entity;
use phalanx_core::{Fixed, FixedVec2, PlayerId};
use phalanx_sim::grid::Cell;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError {
    OutOfBounds,
    Blocked,
    NotEnoughMinerals,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainError {
    QueueFull,
    NotEnoughMinerals,
    SupplyBlocked,
}

/// Hit points of a building `done` of `total` ticks into construction: 10 % at the start, full at the end.
pub(crate) fn build_hp(max: u32, done: u16, total: u16) -> u32 {
    let base = max / 10;
    base + ((max - base) as u64 * done as u64 / total.max(1) as u64) as u32
}

impl DotWars {
    /// Can `player` put a `kind` with its top-left cell on `cell` right now?
    pub fn can_place(
        &self,
        player: PlayerId,
        kind: BuildingKind,
        cell: Cell,
    ) -> Result<(), BuildError> {
        let def = data::building(kind);
        let (w, h) = def.size;
        if self.grid.rect(cell, w, h).is_none() {
            return Err(BuildError::OutOfBounds);
        }
        if !self.grid.rect_all(cell, w, h, |o| o.is_none()) {
            return Err(BuildError::Blocked);
        }
        if self.stock(player) < def.cost {
            return Err(BuildError::NotEnoughMinerals);
        }
        Ok(())
    }

    /// `(used, cap)`. Used counts living units and everything queued in any building,
    /// so two buildings cannot both take the last free supply. Sums, so order does not matter.
    pub fn supply(&self, player: PlayerId) -> (u32, u32) {
        let mut used = 0;
        for (_, (t, k)) in self.world.query::<(&Team, &UnitKind)>().iter() {
            if t.0 == player {
                used += data::unit(*k).supply;
            }
        }
        let mut cap = 0;
        for (_, (t, b, c, p)) in self
            .world
            .query::<(&Team, &Building, Option<&Construction>, Option<&Producer>)>()
            .iter()
        {
            if t.0 != player {
                continue;
            }
            if c.is_none() {
                cap += data::building(b.kind).supply;
            }
            if let Some(p) = p {
                used += p.queue.iter().map(|k| data::unit(*k).supply).sum::<u32>();
            }
        }
        (used, cap.min(SUPPLY_LIMIT))
    }

    /// Spawns a building and takes its cells. Does not check anything: use `can_place` first.
    pub fn spawn_building(
        &mut self,
        kind: BuildingKind,
        cell: Cell,
        owner: PlayerId,
        finished: bool,
    ) -> ObjectId {
        let def = data::building(kind);
        let (w, h) = def.size;
        let id = ObjectId(self.ids.next_id());
        let half = Fixed(w.min(h) as i32 * self.grid.cell_size().raw() / 2);
        let hp = if finished {
            def.hp
        } else {
            build_hp(def.hp, 0, def.build_ticks)
        };
        let e = self.world.spawn((
            id,
            Position(self.grid.rect_center(cell, w, h)),
            Team(owner),
            Body { radius: half },
            Building { kind, cell },
            Health { hp, max: def.hp },
        ));
        self.index.insert(id, e);
        self.grid.fill_rect(cell, w, h, Some(id));
        if finished {
            self.finish_building(e, kind);
        } else {
            let _ = self.world.insert_one(
                e,
                Construction {
                    done: 0,
                    total: def.build_ticks,
                },
            );
        }
        id
    }

    /// Turns a building into a working one: drop the construction state, add what its kind does.
    fn finish_building(&mut self, e: Entity, kind: BuildingKind) {
        let def = data::building(kind);
        let _ = self.world.remove_one::<Construction>(e);
        if !def.trains.is_empty() {
            let _ = self.world.insert_one(e, Producer::default());
        }
        if def.drop_off {
            let _ = self.world.insert_one(e, DropOff);
        }
    }

    /// Own, finished building that can train: its entity.
    fn own_producer(&self, player: PlayerId, building: ObjectId) -> Option<Entity> {
        let e = self.entity(building)?;
        let own = self.world.get::<&Team>(e).is_ok_and(|t| t.0 == player);
        let finished = !self.world.satisfies::<&Construction>(e).unwrap_or(true);
        let producer = self.world.satisfies::<&Producer>(e).unwrap_or(false);
        (own && finished && producer).then_some(e)
    }

    pub(crate) fn train(
        &mut self,
        player: PlayerId,
        building: ObjectId,
        kind: UnitKind,
        events: &mut Vec<Event>,
    ) {
        let Some(e) = self.own_producer(player, building) else {
            return;
        };
        let Some(b) = self.world.get::<&Building>(e).ok().map(|b| b.kind) else {
            return;
        };
        if !data::building(b).trains.contains(&kind) {
            return;
        }
        let def = data::unit(kind);
        let queued = self.world.get::<&Producer>(e).map_or(0, |p| p.queue.len());
        let (used, cap) = self.supply(player);
        let err = if queued >= QUEUE_MAX {
            Some(TrainError::QueueFull)
        } else if self.stock(player) < def.cost {
            Some(TrainError::NotEnoughMinerals)
        } else if used + def.supply > cap {
            Some(TrainError::SupplyBlocked)
        } else {
            None
        };
        if let Some(reason) = err {
            events.push(Event::TrainFailed { player, reason });
            return;
        }
        let stock = self.stock(player);
        self.set_stock(player, stock - def.cost);
        if let Ok(mut p) = self.world.get::<&mut Producer>(e) {
            p.queue.push(kind);
        }
    }

    /// Removes queue item `slot` and refunds it in full.
    pub(crate) fn cancel_train(&mut self, player: PlayerId, building: ObjectId, slot: usize) {
        let Some(e) = self.own_producer(player, building) else {
            return;
        };
        let removed = {
            let Ok(mut p) = self.world.get::<&mut Producer>(e) else {
                return;
            };
            if slot >= p.queue.len() {
                return;
            }
            if slot == 0 {
                p.progress = 0;
            }
            p.queue.remove(slot)
        };
        let stock = self.stock(player);
        self.set_stock(player, stock + data::unit(removed).cost);
    }

    pub(crate) fn set_rally(
        &mut self,
        player: PlayerId,
        buildings: &[ObjectId],
        target: FixedVec2,
    ) {
        let unique: BTreeSet<ObjectId> = buildings.iter().copied().collect();
        for id in unique {
            if let Some(e) = self.own_producer(player, id)
                && let Ok(mut p) = self.world.get::<&mut Producer>(e)
            {
                p.rally = Some(target);
            }
        }
    }

    /// Workers walking to a building site; on arrival the building is placed or the order fails.
    pub(crate) fn run_build(&mut self, events: &mut Vec<Event>) {
        let builders: Vec<(ObjectId, Entity)> = self
            .sorted_objects()
            .into_iter()
            .filter(|(_, e)| {
                self.world
                    .get::<&Orders>(*e)
                    .is_ok_and(|o| matches!(o.current, Some(Order::Build { .. })))
            })
            .collect();
        for (_, e) in builders {
            let (Some(mut pos), Some(team), Some(wr), Some(Order::Build { kind, cell })) = (
                self.world.get::<&Position>(e).ok().map(|p| p.0),
                self.world.get::<&Team>(e).ok().map(|t| t.0),
                self.world.get::<&Body>(e).ok().map(|b| b.radius),
                self.world.get::<&Orders>(e).ok().and_then(|o| o.current),
            ) else {
                continue;
            };
            let (w, h) = data::building(kind).size;
            let mut result = None;
            if self.grid.rect(cell, w, h).is_none() {
                result = Some(Err(BuildError::OutOfBounds));
            } else {
                let center = self.grid.rect_center(cell, w, h);
                let half = Fixed(w.min(h) as i32 * self.grid.cell_size().raw() / 2);
                if step_toward(&mut pos, center, half + wr, WORKER_SPEED) {
                    result = Some(self.can_place(team, kind, cell));
                }
            }
            if let Ok(mut p) = self.world.get::<&mut Position>(e) {
                p.0 = pos;
            }
            let next = match result {
                None => continue,
                Some(Ok(())) => {
                    let stock = self.stock(team);
                    self.set_stock(team, stock - data::building(kind).cost);
                    Some(Order::Construct(
                        self.spawn_building(kind, cell, team, false),
                    ))
                }
                Some(Err(reason)) => {
                    events.push(Event::BuildFailed {
                        player: team,
                        reason,
                    });
                    None
                }
            };
            if let Ok(mut o) = self.world.get::<&mut Orders>(e) {
                o.current = next;
            }
        }
    }

    /// Builders touching an unfinished building add one tick of progress each tick.
    /// One builder per building: the lowest `ObjectId` works, the others wait.
    pub(crate) fn run_construct(&mut self, events: &mut Vec<Event>) {
        let builders: Vec<(ObjectId, Entity)> = self
            .sorted_objects()
            .into_iter()
            .filter(|(_, e)| {
                self.world
                    .get::<&Orders>(*e)
                    .is_ok_and(|o| matches!(o.current, Some(Order::Construct(_))))
            })
            .collect();
        let mut worked: BTreeSet<ObjectId> = BTreeSet::new();
        for (_, e) in builders {
            let (Some(mut pos), Some(wr), Some(Order::Construct(target))) = (
                self.world.get::<&Position>(e).ok().map(|p| p.0),
                self.world.get::<&Body>(e).ok().map(|b| b.radius),
                self.world.get::<&Orders>(e).ok().and_then(|o| o.current),
            ) else {
                continue;
            };
            let site = self.entity(target).and_then(|be| {
                let c = *self.world.get::<&Construction>(be).ok()?;
                let p = self.world.get::<&Position>(be).ok()?.0;
                let r = self.world.get::<&Body>(be).ok()?.radius;
                Some((be, c, p, r))
            });
            let Some((be, c, bpos, br)) = site else {
                // Gone or already finished.
                if let Ok(mut o) = self.world.get::<&mut Orders>(e) {
                    o.current = None;
                }
                continue;
            };
            let arrived = step_toward(&mut pos, bpos, br + wr, WORKER_SPEED);
            if let Ok(mut p) = self.world.get::<&mut Position>(e) {
                p.0 = pos;
            }
            if !arrived || !worked.insert(target) {
                continue;
            }
            let done = c.done + 1;
            if done >= c.total {
                if let Ok(mut h) = self.world.get::<&mut Health>(be)
                    && h.hp > 0
                {
                    let gain = h.max - build_hp(h.max, c.done, c.total).min(h.max);
                    h.hp = (h.hp + gain).min(h.max);
                }
                if let Some(kind) = self.world.get::<&Building>(be).ok().map(|b| b.kind) {
                    self.finish_building(be, kind);
                }
                events.push(Event::BuildingFinished { id: target });
                if let Ok(mut o) = self.world.get::<&mut Orders>(e) {
                    o.current = None;
                }
            } else {
                if let Ok(mut cons) = self.world.get::<&mut Construction>(be) {
                    cons.done = done;
                }
                if let Ok(mut h) = self.world.get::<&mut Health>(be)
                    && h.hp > 0
                {
                    let gain = build_hp(h.max, done, c.total) - build_hp(h.max, c.done, c.total);
                    h.hp = (h.hp + gain).min(h.max);
                }
            }
        }
    }

    /// Advances every finished producer and spawns what completes, in `ObjectId` order.
    pub(crate) fn run_produce(&mut self, events: &mut Vec<Event>) {
        let producers: Vec<(ObjectId, Entity)> = self
            .sorted_objects()
            .into_iter()
            .filter(|(_, e)| {
                self.world.satisfies::<&Producer>(*e).unwrap_or(false)
                    && !self.world.satisfies::<&Construction>(*e).unwrap_or(true)
            })
            .collect();
        for (bid, be) in producers {
            let Some(finished) = ({
                let Ok(mut p) = self.world.get::<&mut Producer>(be) else {
                    continue;
                };
                match p.queue.first().copied() {
                    None => None,
                    Some(kind) => {
                        p.progress += 1;
                        if p.progress >= data::unit(kind).train_ticks {
                            p.progress = 0;
                            p.queue.remove(0);
                            Some((kind, p.rally))
                        } else {
                            None
                        }
                    }
                }
            }) else {
                continue;
            };
            let (kind, rally) = finished;
            let (Some(bpos), Some(br), Some(owner)) = (
                self.world.get::<&Position>(be).ok().map(|p| p.0),
                self.world.get::<&Body>(be).ok().map(|b| b.radius),
                self.world.get::<&Team>(be).ok().map(|t| t.0),
            ) else {
                continue;
            };
            // On the edge facing the rally point; straight down if there is none or it is on the building.
            let reach = br + data::unit(kind).radius + Fixed::ONE;
            let dir = rally
                .map(|r| r - bpos)
                .filter(|d| d.length() > br)
                .unwrap_or(FixedVec2::new(Fixed::ZERO, Fixed::ONE));
            let id = self.spawn_kind(kind, bpos + dir.with_length(reach), owner);
            if let Some(r) = rally
                && let Some(ue) = self.entity(id)
                && let Ok(mut o) = self.world.get::<&mut Orders>(ue)
            {
                o.give(Order::Move(r), false);
            }
            events.push(Event::UnitTrained { id, building: bid });
        }
    }

    /// Workers named in `ids` (others are dropped).
    pub(crate) fn only_workers(&self, ids: &[ObjectId]) -> Vec<ObjectId> {
        ids.iter()
            .copied()
            .filter(|id| {
                self.entity(*id)
                    .is_some_and(|e| self.world.satisfies::<&Worker>(e).unwrap_or(false))
            })
            .collect()
    }
}
