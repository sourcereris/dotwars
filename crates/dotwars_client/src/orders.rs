//! Turns a right click into commands. Pure functions over views: no macroquad input, no sim.

use crate::view::{NodeView, UnitView};
use dotwars_sim::{Command, components::ObjectId};
use macroquad::prelude::Vec2;
use phalanx_client::to_fixed;

/// Nearest node whose body (plus `tolerance` world units) covers `world_pos`.
/// Nodes come sorted by `ObjectId`, so an exact tie goes to the lower id.
pub fn pick_node(nodes: &[NodeView], world_pos: Vec2, tolerance: f32) -> Option<&NodeView> {
    let mut best: Option<(f32, &NodeView)> = None;
    for n in nodes {
        let d = n.pos.distance(world_pos);
        if d <= n.radius + tolerance && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, n));
        }
    }
    best.map(|(_, n)| n)
}

/// Smart right click. On a node: workers gather, everyone else walks there.
/// Anywhere else: everyone moves. `selected` must already be the player's own units.
pub fn right_click_commands(
    views: &[UnitView],
    nodes: &[NodeView],
    selected: &[ObjectId],
    world_pos: Vec2,
    tolerance: f32,
) -> Vec<Command> {
    if selected.is_empty() {
        return Vec::new();
    }
    let Some(node) = pick_node(nodes, world_pos, tolerance) else {
        return vec![Command::Move {
            units: selected.to_vec(),
            target: to_fixed(world_pos),
            queue: false,
        }];
    };
    let is_worker = |id: &ObjectId| views.iter().any(|v| v.id == *id && v.worker);
    let (workers, others): (Vec<_>, Vec<_>) = selected.iter().copied().partition(is_worker);
    let mut cmds = Vec::new();
    if !workers.is_empty() {
        cmds.push(Command::Gather {
            units: workers,
            node: node.id,
            queue: false,
        });
    }
    if !others.is_empty() {
        cmds.push(Command::Move {
            units: others,
            target: to_fixed(node.pos),
            queue: false,
        });
    }
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;
    use phalanx_core::PlayerId;

    fn unit(id: u32, worker: bool) -> UnitView {
        UnitView {
            id: ObjectId(id),
            team: PlayerId(0),
            pos: Vec2::ZERO,
            radius: 8.0,
            worker,
            carrying: false,
        }
    }

    fn node(id: u32, x: f32) -> NodeView {
        NodeView {
            id: ObjectId(id),
            pos: Vec2::new(x, 0.0),
            radius: 12.0,
            amount: 100,
        }
    }

    #[test]
    fn click_on_a_node_splits_workers_from_the_rest() {
        let views = [unit(0, true), unit(1, false), unit(2, true)];
        let nodes = [node(10, 100.0)];
        let sel = [ObjectId(0), ObjectId(1), ObjectId(2)];
        let cmds = right_click_commands(&views, &nodes, &sel, Vec2::new(105.0, 0.0), 0.0);
        assert_eq!(cmds.len(), 2);
        assert!(matches!(
            &cmds[0],
            Command::Gather { units, node, .. }
                if units == &[ObjectId(0), ObjectId(2)] && *node == ObjectId(10)
        ));
        assert!(matches!(
            &cmds[1],
            Command::Move { units, .. } if units == &[ObjectId(1)]
        ));
    }

    #[test]
    fn click_on_ground_moves_everyone() {
        let views = [unit(0, true), unit(1, false)];
        let nodes = [node(10, 100.0)];
        let sel = [ObjectId(0), ObjectId(1)];
        let cmds = right_click_commands(&views, &nodes, &sel, Vec2::new(300.0, 0.0), 0.0);
        assert_eq!(cmds.len(), 1);
        assert!(matches!(&cmds[0], Command::Move { units, .. } if units.len() == 2));
    }

    #[test]
    fn only_workers_selected_sends_just_gather() {
        let views = [unit(0, true)];
        let nodes = [node(10, 100.0)];
        let cmds = right_click_commands(&views, &nodes, &[ObjectId(0)], Vec2::new(100.0, 0.0), 0.0);
        assert!(matches!(cmds.as_slice(), [Command::Gather { .. }]));
    }

    #[test]
    fn empty_selection_sends_nothing() {
        let nodes = [node(10, 100.0)];
        assert!(right_click_commands(&[], &nodes, &[], Vec2::ZERO, 0.0).is_empty());
    }

    #[test]
    fn pick_node_takes_the_nearest_and_misses_far_clicks() {
        let nodes = [node(3, 100.0), node(5, 110.0)];
        assert_eq!(
            pick_node(&nodes, Vec2::new(108.0, 0.0), 0.0).map(|n| n.id),
            Some(ObjectId(5))
        );
        assert_eq!(
            pick_node(&nodes, Vec2::new(101.0, 0.0), 0.0).map(|n| n.id),
            Some(ObjectId(3))
        );
        assert!(pick_node(&nodes, Vec2::new(300.0, 0.0), 0.0).is_none());
    }
}
