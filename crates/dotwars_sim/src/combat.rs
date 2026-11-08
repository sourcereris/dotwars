//! Two-phase combat. Phase 1 decides what every unit does from a snapshot of the start of the tick
//! and writes nothing to the world; phase 2 applies it (moves, then all damage at once). Because
//! nobody sees another unit's hit before the tick ends, two equal units that hit each other on the
//! same tick both die, whatever their ids.

use crate::components::{
    Body, Building, Combat, Health, LastHit, ObjectId, Order, Orders, Position, Team,
};
use crate::data::{self, MELEE, Tag, UnitDef, UnitKind};
use crate::economy::step_toward;
use crate::{DotWars, Event};
use hecs::Entity;
use phalanx_core::{Fixed, FixedVec2, PlayerId};
use std::collections::BTreeMap;

/// How far from its anchor an idle unit chases.
pub const LEASH: Fixed = Fixed::from_int(240);
/// Ticks between re-evaluations of an auto-acquired target.
pub const RETARGET_TICKS: u8 = 10;
/// A unit that hit me within this many ticks is a priority target.
pub const RECENT_TICKS: u32 = 40;
/// Distance buckets for the priority key: one grid cell, as `dist_sq` scales it.
const BUCKET: i64 = (32 * 32) << Fixed::FRAC_BITS;

/// Square of a distance in the scale `dist_sq` uses (Q16.16 in an `i64`, no overflow).
fn sq(f: Fixed) -> i64 {
    let r = f.raw() as i64;
    (r * r) >> Fixed::FRAC_BITS
}

/// Damage one hit of `att` does to something with `tag`.
pub fn damage(att: &UnitDef, tag: Tag) -> u32 {
    let bonus = match att.bonus {
        Some((t, extra)) if t == tag => extra,
        _ => 0,
    };
    (att.damage + bonus).max(1)
}

/// Start-of-tick view of one object.
struct Snap {
    pos: FixedVec2,
    radius: Fixed,
    team: PlayerId,
    hp: u32,
    /// Armed unit: targeted before workers and buildings.
    threat: bool,
    tag: Tag,
}

enum OnArrive {
    Nothing,
    FinishOrder,
    ClearAnchor,
}

enum Act {
    None,
    Hit {
        target: ObjectId,
        dmg: u32,
        ranged: bool,
    },
    Step {
        to: FixedVec2,
        stop_at: Fixed,
        on_arrive: OnArrive,
    },
    /// The order is over (target gone).
    Finish,
}

struct Plan {
    id: ObjectId,
    e: Entity,
    speed: Fixed,
    combat: Combat,
    act: Act,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Explicit(ObjectId),
    AttackMove(FixedVec2),
    Hold,
    Idle,
}

impl DotWars {
    /// Units named in `ids` that can fight (others are dropped).
    pub(crate) fn only_fighters(&self, ids: &[ObjectId]) -> Vec<ObjectId> {
        ids.iter()
            .copied()
            .filter(|id| {
                self.entity(*id)
                    .is_some_and(|e| self.world.satisfies::<&Combat>(e).unwrap_or(false))
            })
            .collect()
    }

    fn snapshot(&self) -> BTreeMap<ObjectId, Snap> {
        let mut out = BTreeMap::new();
        for (id, e) in self.sorted_objects() {
            let (Ok(h), Ok(t), Ok(p), Ok(b)) = (
                self.world.get::<&Health>(e),
                self.world.get::<&Team>(e),
                self.world.get::<&Position>(e),
                self.world.get::<&Body>(e),
            ) else {
                continue;
            };
            let (tag, threat) = match self.world.get::<&UnitKind>(e) {
                Ok(k) => (data::unit(*k).tag, *k != UnitKind::Worker),
                Err(_) => (Tag::Structure, false),
            };
            out.insert(
                id,
                Snap {
                    pos: p.0,
                    radius: b.radius,
                    team: t.0,
                    hp: h.hp,
                    threat,
                    tag,
                },
            );
        }
        out
    }

    /// Phase 1 for one unit. Reads the snapshot and its own components only.
    fn decide(&self, id: ObjectId, e: Entity, snaps: &BTreeMap<ObjectId, Snap>) -> Option<Plan> {
        let me = snaps.get(&id)?;
        let kind = *self.world.get::<&UnitKind>(e).ok()?;
        let def = data::unit(kind);
        let mut c = *self.world.get::<&Combat>(e).ok()?;
        c.cooldown.tick();
        let cur = self.world.get::<&Orders>(e).ok()?.current;
        let last_hit = self.world.get::<&LastHit>(e).ok().map(|l| *l);
        let plan = |c: Combat, act: Act| {
            Some(Plan {
                id,
                e,
                speed: def.speed,
                combat: c,
                act,
            })
        };

        let mode = match cur {
            Some(Order::Attack(t)) => Mode::Explicit(t),
            Some(Order::AttackMove(p)) => Mode::AttackMove(p),
            Some(Order::Hold) => Mode::Hold,
            None => Mode::Idle,
            Some(_) => {
                // Moving, gathering, building: not fighting.
                c.target = None;
                c.anchor = None;
                return plan(c, Act::None);
            }
        };

        let reach = |t: &Snap| def.range + me.radius + t.radius;
        let in_range = |t: &Snap| me.pos.dist_sq(t.pos) <= sq(reach(t));
        let engage = |c: &mut Combat, t: &Snap, tid: ObjectId, may_move: bool| -> Act {
            if in_range(t) {
                if c.cooldown.ready() {
                    c.cooldown.start(def.cooldown);
                    Act::Hit {
                        target: tid,
                        dmg: damage(def, t.tag),
                        ranged: def.range > MELEE,
                    }
                } else {
                    Act::None
                }
            } else if may_move {
                // One unit short of the range, so rounding can't leave us just outside it.
                Act::Step {
                    to: t.pos,
                    stop_at: (reach(t) - Fixed::from_int(1)).max(Fixed::ZERO),
                    on_arrive: OnArrive::Nothing,
                }
            } else {
                Act::None
            }
        };

        if let Mode::Explicit(tid) = mode {
            let Some(t) = snaps.get(&tid).filter(|t| t.team != me.team) else {
                c.target = None;
                return plan(c, Act::Finish);
            };
            c.target = Some(tid);
            let act = engage(&mut c, t, tid, true);
            return plan(c, act);
        }

        // Auto-acquire.
        let anchor = c.anchor;
        let valid = |t: &Snap| {
            t.team != me.team
                && match mode {
                    Mode::Hold => in_range(t),
                    Mode::Idle => anchor.is_none_or(|a| a.dist_sq(t.pos) <= sq(LEASH)),
                    _ => true,
                }
        };
        let eligible = |t: &Snap| {
            valid(t)
                && def
                    .acquire
                    .is_some_and(|a| mode == Mode::Hold || me.pos.dist_sq(t.pos) <= sq(a))
        };
        let key = |tid: ObjectId, t: &Snap| {
            let recent = last_hit
                .is_some_and(|l| l.by == tid && self.tick.saturating_sub(l.tick) <= RECENT_TICKS);
            (
                !recent,
                !t.threat,
                me.pos.dist_sq(t.pos) / BUCKET,
                t.hp,
                tid,
            )
        };

        let current = c
            .target
            .and_then(|tid| snaps.get(&tid).map(|t| (tid, t)))
            .filter(|(_, t)| valid(t) && def.acquire.is_some());
        let chosen = if current.is_none() || c.retarget_in == 0 {
            c.retarget_in = RETARGET_TICKS;
            let best = snaps
                .iter()
                .filter(|(_, t)| eligible(t))
                .min_by_key(|(tid, t)| key(**tid, t))
                .map(|(tid, t)| (*tid, t));
            match (current, best) {
                (Some((cid, ct)), Some((bid, bt))) if bid != cid => {
                    let (kc, kb) = (key(cid, ct), key(bid, bt));
                    if (kb.0, kb.1) < (kc.0, kc.1) {
                        Some((bid, bt))
                    } else {
                        current
                    }
                }
                (Some(cur), _) => Some(cur),
                (None, best) => best,
            }
        } else {
            c.retarget_in -= 1;
            current
        };

        match chosen {
            Some((tid, t)) => {
                c.target = Some(tid);
                if mode == Mode::Idle && c.anchor.is_none() {
                    c.anchor = Some(me.pos);
                }
                let act = engage(&mut c, t, tid, mode != Mode::Hold);
                plan(c, act)
            }
            None => {
                c.target = None;
                let act = match mode {
                    Mode::Idle => match c.anchor {
                        Some(a) => Act::Step {
                            to: a,
                            stop_at: Fixed::ZERO,
                            on_arrive: OnArrive::ClearAnchor,
                        },
                        None => Act::None,
                    },
                    Mode::AttackMove(dest) => Act::Step {
                        to: dest,
                        stop_at: Fixed::ZERO,
                        on_arrive: OnArrive::FinishOrder,
                    },
                    _ => Act::None,
                };
                plan(c, act)
            }
        }
    }

    pub(crate) fn run_combat(&mut self, events: &mut Vec<Event>) {
        let snaps = self.snapshot();
        let plans: Vec<Plan> = self
            .sorted_units()
            .into_iter()
            .filter_map(|(id, e)| self.decide(id, e, &snaps))
            .collect();

        // Phase 2. Moves touch only their own unit; damage is summed and applied afterwards.
        let mut dealt: BTreeMap<ObjectId, (u32, ObjectId)> = BTreeMap::new();
        for p in plans {
            if let Ok(mut c) = self.world.get::<&mut Combat>(p.e) {
                *c = p.combat;
            }
            match p.act {
                Act::None => {}
                Act::Hit {
                    target,
                    dmg,
                    ranged,
                } => {
                    events.push(Event::Attacked {
                        attacker: p.id,
                        target,
                        ranged,
                    });
                    // Ids are walked in order, so the lowest attacker is the one remembered.
                    dealt.entry(target).or_insert((0, p.id)).0 += dmg;
                }
                Act::Step {
                    to,
                    stop_at,
                    on_arrive,
                } => {
                    let Some(mut pos) = self.world.get::<&Position>(p.e).ok().map(|q| q.0) else {
                        continue;
                    };
                    let arrived = step_toward(&mut pos, to, stop_at, p.speed);
                    if let Ok(mut q) = self.world.get::<&mut Position>(p.e) {
                        q.0 = pos;
                    }
                    if arrived {
                        match on_arrive {
                            OnArrive::Nothing => {}
                            OnArrive::FinishOrder => {
                                if let Ok(mut o) = self.world.get::<&mut Orders>(p.e) {
                                    o.current = None;
                                }
                            }
                            OnArrive::ClearAnchor => {
                                if let Ok(mut c) = self.world.get::<&mut Combat>(p.e) {
                                    c.anchor = None;
                                }
                            }
                        }
                    }
                }
                Act::Finish => {
                    if let Ok(mut o) = self.world.get::<&mut Orders>(p.e) {
                        o.current = None;
                    }
                }
            }
        }
        for (target, (amount, by)) in dealt {
            let Some(e) = self.entity(target) else {
                continue;
            };
            if let Ok(mut h) = self.world.get::<&mut Health>(e) {
                h.hp = h.hp.saturating_sub(amount);
            }
            if self.world.satisfies::<&Combat>(e).unwrap_or(false) {
                let _ = self.world.insert_one(
                    e,
                    LastHit {
                        by,
                        tick: self.tick,
                    },
                );
            }
        }
    }

    /// Removes everything at 0 hp, in `ObjectId` order. `despawn` frees the index and the grid.
    pub(crate) fn run_death(&mut self, events: &mut Vec<Event>) {
        let dead: Vec<(ObjectId, Entity)> = self
            .sorted_objects()
            .into_iter()
            .filter(|(_, e)| self.world.get::<&Health>(*e).is_ok_and(|h| h.hp == 0))
            .collect();
        for (id, e) in dead {
            let owner = self.world.get::<&Team>(e).map(|t| t.0);
            let pos = self.world.get::<&Position>(e).map(|p| p.0);
            let building = self.world.satisfies::<&Building>(e).unwrap_or(false);
            if let (Ok(owner), Ok(pos)) = (owner, pos) {
                events.push(Event::Died {
                    id,
                    owner,
                    pos,
                    building,
                });
            }
            self.despawn(id);
        }
    }

    /// Test helper: take `amount` hp off `id`.
    #[cfg(test)]
    pub(crate) fn hurt(&mut self, id: ObjectId, amount: u32) {
        if let Some(e) = self.entity(id)
            && let Ok(mut h) = self.world.get::<&mut Health>(e)
        {
            h.hp = h.hp.saturating_sub(amount);
        }
    }
}
