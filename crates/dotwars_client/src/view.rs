//! What the client needs to know about units, copied out of the sim each frame with
//! **interpolated** positions. Picking and drawing work on `&[UnitView]`, never on the sim.

use dotwars_sim::{
    DotWars,
    components::{Body, Position, PrevPosition, Team, UnitId},
};
use macroquad::prelude::Vec2;
use phalanx_client::lerp_pos;
use phalanx_core::PlayerId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitView {
    pub id: UnitId,
    pub team: PlayerId,
    /// World position, interpolated between the last two ticks.
    pub pos: Vec2,
    pub radius: f32,
}

/// All units, sorted by `UnitId`.
pub fn build_views(sim: &DotWars, alpha: f32) -> Vec<UnitView> {
    sim.sorted_units()
        .into_iter()
        .filter_map(|(id, e)| {
            let pos = sim.world.get::<&Position>(e).ok()?.0;
            let prev = sim.world.get::<&PrevPosition>(e).ok()?.0;
            let team = sim.world.get::<&Team>(e).ok()?.0;
            let radius = sim.world.get::<&Body>(e).ok()?.radius.to_f32();
            Some(UnitView {
                id,
                team,
                pos: lerp_pos(prev, pos, alpha),
                radius,
            })
        })
        .collect()
}
