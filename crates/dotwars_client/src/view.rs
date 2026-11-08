//! What the client needs to know about the sim, copied out each frame.
//! Units carry **interpolated** positions. Picking and drawing work on these views, never on the sim.

use dotwars_sim::{
    DotWars,
    components::{
        Body, Building, Construction, DropOff, ObjectId, Position, PrevPosition, Producer,
        ResourceNode, Team, Worker,
    },
    data::{self, BuildingKind, UnitKind},
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

/// A building with everything the UI shows about it.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildingView {
    pub id: ObjectId,
    pub team: PlayerId,
    pub kind: BuildingKind,
    /// Footprint center.
    pub pos: Vec2,
    /// Footprint size in world units.
    pub size: Vec2,
    /// `Some(0..1)` while under construction.
    pub construction: Option<f32>,
    pub queue: Vec<UnitKind>,
    /// Progress of the front queue item, 0..1.
    pub train_progress: f32,
    pub rally: Option<Vec2>,
}

impl BuildingView {
    pub fn finished(&self) -> bool {
        self.construction.is_none()
    }

    pub fn trains(&self) -> &'static [UnitKind] {
        data::building(self.kind).trains
    }

    pub fn contains(&self, p: Vec2) -> bool {
        let d = (p - self.pos).abs();
        d.x <= self.size.x / 2.0 && d.y <= self.size.y / 2.0
    }
}

/// All buildings, sorted by `ObjectId`.
pub fn build_buildings(sim: &DotWars) -> Vec<BuildingView> {
    let cell = dotwars_sim::CELL_SIZE.to_f32();
    sim.sorted_objects()
        .into_iter()
        .filter_map(|(id, e)| {
            let b = *sim.world.get::<&Building>(e).ok()?;
            let team = sim.world.get::<&Team>(e).ok()?.0;
            let p = sim.world.get::<&Position>(e).ok()?.0.to_f32();
            let (w, h) = data::building(b.kind).size;
            let construction = sim
                .world
                .get::<&Construction>(e)
                .ok()
                .map(|c| c.done as f32 / c.total.max(1) as f32);
            let (queue, train_progress, rally) = match sim.world.get::<&Producer>(e) {
                Ok(pr) => {
                    let frac = pr.queue.first().map_or(0.0, |k| {
                        pr.progress as f32 / data::unit(*k).train_ticks as f32
                    });
                    (
                        pr.queue.clone(),
                        frac,
                        pr.rally.map(|r| {
                            let (x, y) = r.to_f32();
                            Vec2::new(x, y)
                        }),
                    )
                }
                Err(_) => (Vec::new(), 0.0, None),
            };
            Some(BuildingView {
                id,
                team,
                kind: b.kind,
                pos: Vec2::new(p.0, p.1),
                size: Vec2::new(w as f32 * cell, h as f32 * cell),
                construction,
                queue,
                train_progress,
                rally,
            })
        })
        .collect()
}
