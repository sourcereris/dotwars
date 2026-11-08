//! Dot Wars rules. Headless: no macroquad, no floats, no clock.
#![deny(clippy::disallowed_types, clippy::disallowed_methods)]

pub mod components;
mod systems;

use components::{MoveTarget, Position, PrevPosition, UnitId};
use hecs::{Entity, World};
use phalanx_core::{Checksum, FixedVec2, IdGen, PlayerId, Rng};
use phalanx_sim::Simulation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Units are named by stable `UnitId`, never by `hecs::Entity`.
    Move {
        units: Vec<UnitId>,
        target: FixedVec2,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {}

pub struct DotWars {
    pub world: World,
    pub rng: Rng,
    ids: IdGen,
}

impl DotWars {
    pub fn new(seed: u64) -> Self {
        Self {
            world: World::new(),
            rng: Rng::new(seed),
            ids: IdGen::default(),
        }
    }

    pub fn spawn_unit(&mut self, pos: FixedVec2) -> UnitId {
        let id = UnitId(self.ids.next_id());
        self.world.spawn((id, Position(pos), PrevPosition(pos)));
        id
    }

    /// All units sorted by `UnitId`. Use whenever iteration order affects the outcome.
    pub fn sorted_units(&self) -> Vec<(UnitId, Entity)> {
        let mut v: Vec<_> = self
            .world
            .query::<&UnitId>()
            .iter()
            .map(|(e, id)| (*id, e))
            .collect();
        v.sort_unstable_by_key(|(id, _)| *id);
        v
    }

    fn apply(&mut self, _player: PlayerId, cmd: &Command) {
        match cmd {
            Command::Move { units, target } => {
                for (id, e) in self.sorted_units() {
                    if units.contains(&id) {
                        // Entity may be gone: insert just fails, which is fine.
                        let _ = self.world.insert_one(e, MoveTarget(*target));
                    }
                }
            }
        }
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
        systems::movement(&mut self.world);
    }

    fn checksum(&self) -> u64 {
        let mut h = Checksum::default();
        let (s, i) = self.rng.state();
        h.u64(s);
        h.u64(i);
        for (id, e) in self.sorted_units() {
            h.u32(id.0);
            if let Ok(p) = self.world.get::<&Position>(e) {
                h.i32(p.0.x.raw());
                h.i32(p.0.y.raw());
            }
        }
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phalanx_sim::Driver;

    fn run(seed: u64) -> (Vec<u64>, FixedVec2) {
        let mut sim = DotWars::new(seed);
        let a = sim.spawn_unit(FixedVec2::ZERO);
        let mut d = Driver::new(sim, 3);
        let target = FixedVec2::from_ints(100, 50);
        d.issue(
            PlayerId(0),
            Command::Move {
                units: vec![a],
                target,
            },
        );
        let mut ev = Vec::new();
        let sums = (0..200)
            .map(|_| {
                d.step(&mut ev);
                d.sim.checksum()
            })
            .collect();
        let (_, e) = d.sim.sorted_units()[0];
        let pos = d.sim.world.get::<&Position>(e).unwrap().0;
        (sums, pos)
    }

    #[test]
    fn determinism_same_commands_same_checksums() {
        assert_eq!(run(7).0, run(7).0);
    }

    #[test]
    fn unit_reaches_target() {
        assert_eq!(run(7).1, FixedVec2::from_ints(100, 50));
    }
}
