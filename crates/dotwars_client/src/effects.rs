//! Client-only feedback made from sim events. Real time and floats are fine here:
//! nothing in this file ever reaches the sim.

use crate::view::{BuildingView, DropOffView, UnitView};
use dotwars_sim::{BuildError, Event, TrainError, components::ObjectId};
use macroquad::prelude::Vec2;
use phalanx_core::PlayerId;

const TEXT_SECS: f32 = 1.0;
const RING_SECS: f32 = 0.5;
const FLASH_SECS: f32 = 0.6;
const BANNER_SECS: f32 = 2.0;
const BURST_SECS: f32 = 0.5;
const TRACER_SECS: f32 = 0.1;
const HIT_SECS: f32 = 0.15;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// "+5" rising over a drop-off.
    FloatText(String),
    /// Growing, fading ring where a node ran dry.
    Ring,
    /// White flash over a footprint of this size (a finished building).
    Flash(Vec2),
    /// Fading circle (a unit died) or square (a building died) of this radius.
    Burst { radius: f32, square: bool },
    /// Line from `pos` to this point (a ranged hit).
    Tracer(Vec2),
    /// Small flash where a melee hit landed.
    Hit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub pos: Vec2,
    pub kind: Kind,
    pub age: f32,
}

impl Effect {
    fn lifetime(&self) -> f32 {
        match self.kind {
            Kind::FloatText(_) => TEXT_SECS,
            Kind::Ring => RING_SECS,
            Kind::Flash(_) => FLASH_SECS,
            Kind::Burst { .. } => BURST_SECS,
            Kind::Tracer(_) => TRACER_SECS,
            Kind::Hit => HIT_SECS,
        }
    }

    /// 0 at birth, 1 at the end.
    pub fn progress(&self) -> f32 {
        (self.age / self.lifetime()).clamp(0.0, 1.0)
    }
}

#[derive(Default)]
pub struct Effects {
    pub list: Vec<Effect>,
    /// Red message in the middle of the screen and its age.
    pub banner: Option<(&'static str, f32)>,
}

fn message(ev: &Event, local: PlayerId) -> Option<&'static str> {
    match ev {
        Event::BuildFailed { player, reason } if *player == local => Some(match reason {
            BuildError::OutOfBounds | BuildError::Blocked => "Can't build there",
            BuildError::NotEnoughMinerals => "Not enough minerals",
        }),
        Event::TrainFailed { player, reason } if *player == local => Some(match reason {
            TrainError::QueueFull => "Queue full",
            TrainError::NotEnoughMinerals => "Not enough minerals",
            TrainError::SupplyBlocked => "Supply blocked",
        }),
        _ => None,
    }
}

impl Effects {
    /// `local` only sees its own deposits.
    pub fn on_events(
        &mut self,
        events: &[Event],
        local: PlayerId,
        views: &[UnitView],
        dropoffs: &[DropOffView],
        buildings: &[BuildingView],
    ) {
        // Where something is now. A mutual kill leaves nothing to point at, so those effects are skipped.
        let locate = |id: ObjectId| {
            views
                .iter()
                .find(|v| v.id == id)
                .map(|v| v.pos)
                .or_else(|| buildings.iter().find(|b| b.id == id).map(|b| b.pos))
        };
        for ev in events {
            if let Some(text) = message(ev, local) {
                self.banner = Some((text, 0.0));
            }
            match ev {
                Event::Deposited {
                    player,
                    dropoff,
                    amount,
                } if *player == local => {
                    if let Some(d) = dropoffs.iter().find(|d| d.id == *dropoff) {
                        self.list.push(Effect {
                            pos: d.pos - Vec2::new(0.0, d.radius),
                            kind: Kind::FloatText(format!("+{amount}")),
                            age: 0.0,
                        });
                    }
                }
                Event::NodeDepleted { pos, .. } => {
                    let (x, y) = pos.to_f32();
                    self.list.push(Effect {
                        pos: Vec2::new(x, y),
                        kind: Kind::Ring,
                        age: 0.0,
                    });
                }
                Event::BuildingFinished { id } => {
                    if let Some(b) = buildings.iter().find(|b| b.id == *id) {
                        self.list.push(Effect {
                            pos: b.pos,
                            kind: Kind::Flash(b.size),
                            age: 0.0,
                        });
                    }
                }
                Event::Attacked {
                    attacker,
                    target,
                    ranged,
                } => {
                    if let (Some(from), Some(to)) = (locate(*attacker), locate(*target)) {
                        let (pos, kind) = if *ranged {
                            (from, Kind::Tracer(to))
                        } else {
                            (to, Kind::Hit)
                        };
                        self.list.push(Effect {
                            pos,
                            kind,
                            age: 0.0,
                        });
                    }
                }
                Event::Died { pos, building, .. } => {
                    let (x, y) = pos.to_f32();
                    self.list.push(Effect {
                        pos: Vec2::new(x, y),
                        kind: Kind::Burst {
                            radius: if *building { 40.0 } else { 12.0 },
                            square: *building,
                        },
                        age: 0.0,
                    });
                }
                _ => {}
            }
        }
    }

    pub fn update(&mut self, dt: f32) {
        if let Some((_, age)) = &mut self.banner {
            *age += dt;
        }
        if self.banner.is_some_and(|(_, age)| age >= BANNER_SECS) {
            self.banner = None;
        }
        for e in &mut self.list {
            e.age += dt;
        }
        self.list.retain(|e| e.age < e.lifetime());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotwars_sim::components::ObjectId;
    use phalanx_core::FixedVec2;

    fn dropoff() -> DropOffView {
        DropOffView {
            id: ObjectId(0),
            team: PlayerId(0),
            pos: Vec2::new(10.0, 10.0),
            radius: 24.0,
        }
    }

    fn deposit(player: u8) -> Event {
        Event::Deposited {
            player: PlayerId(player),
            dropoff: ObjectId(0),
            amount: 5,
        }
    }

    #[test]
    fn only_own_deposits_show_text() {
        let mut fx = Effects::default();
        fx.on_events(&[deposit(1)], PlayerId(0), &[], &[dropoff()], &[]);
        assert!(fx.list.is_empty());
        fx.on_events(&[deposit(0)], PlayerId(0), &[], &[dropoff()], &[]);
        assert_eq!(fx.list.len(), 1);
        assert_eq!(fx.list[0].kind, Kind::FloatText("+5".into()));
    }

    #[test]
    fn effects_expire() {
        let mut fx = Effects::default();
        fx.on_events(
            &[Event::NodeDepleted {
                node: ObjectId(1),
                pos: FixedVec2::from_ints(5, 5),
            }],
            PlayerId(0),
            &[],
            &[],
            &[],
        );
        fx.update(RING_SECS * 0.5);
        assert_eq!(fx.list.len(), 1);
        assert!((fx.list[0].progress() - 0.5).abs() < 1e-6);
        fx.update(RING_SECS);
        assert!(fx.list.is_empty());
    }

    #[test]
    fn only_my_failures_show_a_banner_and_it_expires() {
        let mut fx = Effects::default();
        let fail = |player| Event::BuildFailed {
            player: PlayerId(player),
            reason: BuildError::NotEnoughMinerals,
        };
        fx.on_events(&[fail(1)], PlayerId(0), &[], &[], &[]);
        assert!(fx.banner.is_none());
        fx.on_events(&[fail(0)], PlayerId(0), &[], &[], &[]);
        assert_eq!(fx.banner.map(|b| b.0), Some("Not enough minerals"));
        fx.update(BANNER_SECS + 0.1);
        assert!(fx.banner.is_none());
    }

    fn view(id: u32, x: f32) -> UnitView {
        UnitView {
            id: ObjectId(id),
            team: PlayerId(0),
            pos: Vec2::new(x, 0.0),
            radius: 8.0,
            worker: false,
            carrying: false,
            hp: (1, 1),
        }
    }

    #[test]
    fn ranged_hits_draw_a_tracer_and_melee_hits_a_flash_on_the_target() {
        let views = [view(1, 0.0), view(2, 50.0)];
        let hit = |ranged| Event::Attacked {
            attacker: ObjectId(1),
            target: ObjectId(2),
            ranged,
        };
        let mut fx = Effects::default();
        fx.on_events(&[hit(true), hit(false)], PlayerId(0), &views, &[], &[]);
        assert_eq!(fx.list[0].pos, Vec2::new(0.0, 0.0));
        assert_eq!(fx.list[0].kind, Kind::Tracer(Vec2::new(50.0, 0.0)));
        assert_eq!(fx.list[1].pos, Vec2::new(50.0, 0.0));
        assert_eq!(fx.list[1].kind, Kind::Hit);
        fx.update(TRACER_SECS + 0.01);
        assert_eq!(
            fx.list.len(),
            1,
            "the tracer is gone, the flash lives a bit longer"
        );
        fx.update(HIT_SECS);
        assert!(fx.list.is_empty());
    }

    #[test]
    fn effects_for_vanished_objects_are_skipped() {
        let mut fx = Effects::default();
        let ev = Event::Attacked {
            attacker: ObjectId(1),
            target: ObjectId(2),
            ranged: true,
        };
        fx.on_events(&[ev], PlayerId(0), &[view(1, 0.0)], &[], &[]);
        assert!(fx.list.is_empty());
    }

    #[test]
    fn deaths_burst_as_a_circle_for_units_and_a_square_for_buildings() {
        let died = |building| Event::Died {
            id: ObjectId(1),
            owner: PlayerId(1),
            pos: FixedVec2::from_ints(7, 9),
            building,
        };
        let mut fx = Effects::default();
        fx.on_events(&[died(false), died(true)], PlayerId(0), &[], &[], &[]);
        assert!(matches!(fx.list[0].kind, Kind::Burst { square: false, .. }));
        assert!(matches!(fx.list[1].kind, Kind::Burst { square: true, .. }));
        fx.update(BURST_SECS + 0.01);
        assert!(fx.list.is_empty());
    }
}
