//! Client-only feedback made from sim events. Real time and floats are fine here:
//! nothing in this file ever reaches the sim.

use crate::view::DropOffView;
use dotwars_sim::Event;
use macroquad::prelude::Vec2;
use phalanx_core::PlayerId;

const TEXT_SECS: f32 = 1.0;
const RING_SECS: f32 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// "+5" rising over a drop-off.
    FloatText(String),
    /// Growing, fading ring where a node ran dry.
    Ring,
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
}

impl Effects {
    /// `local` only sees its own deposits.
    pub fn on_events(&mut self, events: &[Event], local: PlayerId, dropoffs: &[DropOffView]) {
        for ev in events {
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
                _ => {}
            }
        }
    }

    pub fn update(&mut self, dt: f32) {
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
        fx.on_events(&[deposit(1)], PlayerId(0), &[dropoff()]);
        assert!(fx.list.is_empty());
        fx.on_events(&[deposit(0)], PlayerId(0), &[dropoff()]);
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
        );
        fx.update(RING_SECS * 0.5);
        assert_eq!(fx.list.len(), 1);
        assert!((fx.list[0].progress() - 0.5).abs() < 1e-6);
        fx.update(RING_SECS);
        assert!(fx.list.is_empty());
    }
}
