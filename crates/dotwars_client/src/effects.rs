//! Client-only feedback made from sim events. Real time and floats are fine here:
//! nothing in this file ever reaches the sim.

use crate::view::{BuildingView, DropOffView};
use dotwars_sim::{BuildError, Event, TrainError};
use macroquad::prelude::Vec2;
use phalanx_core::PlayerId;

const TEXT_SECS: f32 = 1.0;
const RING_SECS: f32 = 0.5;
const FLASH_SECS: f32 = 0.6;
const BANNER_SECS: f32 = 2.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// "+5" rising over a drop-off.
    FloatText(String),
    /// Growing, fading ring where a node ran dry.
    Ring,
    /// White flash over a footprint of this size (a finished building).
    Flash(Vec2),
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
        dropoffs: &[DropOffView],
        buildings: &[BuildingView],
    ) {
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
        fx.on_events(&[deposit(1)], PlayerId(0), &[dropoff()], &[]);
        assert!(fx.list.is_empty());
        fx.on_events(&[deposit(0)], PlayerId(0), &[dropoff()], &[]);
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
        fx.on_events(&[fail(1)], PlayerId(0), &[], &[]);
        assert!(fx.banner.is_none());
        fx.on_events(&[fail(0)], PlayerId(0), &[], &[]);
        assert_eq!(fx.banner.map(|b| b.0), Some("Not enough minerals"));
        fx.update(BANNER_SECS + 0.1);
        assert!(fx.banner.is_none());
    }
}
