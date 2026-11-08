//! What the client needs to know about the sim, copied out each frame.
//! Units carry **interpolated** positions. Picking and drawing work on these views, never on the sim.

use dotwars_sim::{
    DotWars,
    components::{Body, DropOff, ObjectId, Position, PrevPosition, ResourceNode, Team, Worker},
};
use macroquad::prelude::Vec2;
use phalanx_client::lerp_pos;
use phalanx_core::PlayerId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitView {
    pub id: ObjectId,
    pub team: PlayerId,
    /// World position, interpolated between the last two ticks.
    pub pos: Vec2,
    pub radius: f32,
    pub worker: bool,
    /// A worker holding minerals.
    pub carrying: bool,
}

/// A resource node. Static, so no interpolation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeView {
    pub id: ObjectId,
    pub pos: Vec2,
    pub radius: f32,
    pub amount: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropOffView {
    pub id: ObjectId,
    pub team: PlayerId,
    pub pos: Vec2,
    pub radius: f32,
}

/// All units, sorted by `ObjectId`.
pub fn build_views(sim: &DotWars, alpha: f32) -> Vec<UnitView> {
    sim.sorted_units()
        .into_iter()
        .filter_map(|(id, e)| {
            let pos = sim.world.get::<&Position>(e).ok()?.0;
            let prev = sim.world.get::<&PrevPosition>(e).ok()?.0;
            let team = sim.world.get::<&Team>(e).ok()?.0;
            let radius = sim.world.get::<&Body>(e).ok()?.radius.to_f32();
            let worker = sim.world.get::<&Worker>(e).ok().map(|w| *w);
            Some(UnitView {
                id,
                team,
                pos: lerp_pos(prev, pos, alpha),
                radius,
                worker: worker.is_some(),
                carrying: worker.is_some_and(|w| w.carry > 0),
            })
        })
        .collect()
}

/// All resource nodes, sorted by `ObjectId`.
pub fn build_nodes(sim: &DotWars) -> Vec<NodeView> {
    sim.sorted_objects()
        .into_iter()
        .filter_map(|(id, e)| {
            let amount = sim.world.get::<&ResourceNode>(e).ok()?.amount;
            let p = sim.world.get::<&Position>(e).ok()?.0.to_f32();
            let radius = sim.world.get::<&Body>(e).ok()?.radius.to_f32();
            Some(NodeView {
                id,
                pos: Vec2::new(p.0, p.1),
                radius,
                amount,
            })
        })
        .collect()
}

/// All drop-offs, sorted by `ObjectId`.
pub fn build_dropoffs(sim: &DotWars) -> Vec<DropOffView> {
    sim.sorted_objects()
        .into_iter()
        .filter_map(|(id, e)| {
            sim.world.get::<&DropOff>(e).ok()?;
            let team = sim.world.get::<&Team>(e).ok()?.0;
            let p = sim.world.get::<&Position>(e).ok()?.0.to_f32();
            let radius = sim.world.get::<&Body>(e).ok()?.radius.to_f32();
            Some(DropOffView {
                id,
                team,
                pos: Vec2::new(p.0, p.1),
                radius,
            })
        })
        .collect()
}
