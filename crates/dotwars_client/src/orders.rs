//! Turns clicks and keys into commands. Pure functions over views: no macroquad input, no sim.

use crate::view::{BuildingView, NodeView, UnitView};
use dotwars_sim::{
    CELL_SIZE, Command,
    components::ObjectId,
    data::{self, BuildingKind},
};
use macroquad::prelude::Vec2;
use phalanx_client::to_fixed;
use phalanx_core::PlayerId;
use phalanx_sim::grid::Cell;

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

/// The building under the cursor. Footprints never overlap, so there is at most one.
pub fn pick_building(buildings: &[BuildingView], world_pos: Vec2) -> Option<&BuildingView> {
    buildings.iter().find(|b| b.contains(world_pos))
}

/// The enemy under `world_pos`: the nearest unit whose body (plus `tolerance`) covers it, else a building.
pub fn pick_enemy(t: &Targets, world_pos: Vec2, tolerance: f32) -> Option<ObjectId> {
    let mut best: Option<(f32, ObjectId)> = None;
    for v in t.views.iter().filter(|v| v.team != t.local) {
        let d = v.pos.distance(world_pos);
        if d <= v.radius + tolerance && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, v.id));
        }
    }
    best.map(|(_, id)| id).or_else(|| {
        pick_building(t.buildings, world_pos)
            .filter(|b| b.team != t.local)
            .map(|b| b.id)
    })
}

/// Left click in attack mode: an enemy under the cursor is attacked, anything else is attack-moved to.
pub fn attack_click_command(
    t: &Targets,
    selected: &[ObjectId],
    world_pos: Vec2,
    tolerance: f32,
    queue: bool,
) -> Option<Command> {
    if selected.is_empty() {
        return None;
    }
    Some(match pick_enemy(t, world_pos, tolerance) {
        Some(target) => Command::Attack {
            units: selected.to_vec(),
            target,
            queue,
        },
        None => Command::AttackMove {
            units: selected.to_vec(),
            target: to_fixed(world_pos),
            queue,
        },
    })
}

/// What a right click can hit.
pub struct Targets<'a> {
    pub views: &'a [UnitView],
    pub nodes: &'a [NodeView],
    pub buildings: &'a [BuildingView],
    pub local: PlayerId,
}

/// Smart right click for selected units (`selected` must already be the player's own).
/// - Enemy unit or building: everyone attacks it.
/// - Resource node: workers gather, everyone else walks there.
/// - Own unfinished building: workers construct, everyone else walks there.
/// - Anywhere else: everyone moves.
pub fn right_click_commands(
    t: &Targets,
    selected: &[ObjectId],
    world_pos: Vec2,
    tolerance: f32,
) -> Vec<Command> {
    if selected.is_empty() {
        return Vec::new();
    }
    if let Some(target) = pick_enemy(t, world_pos, tolerance) {
        return vec![Command::Attack {
            units: selected.to_vec(),
            target,
            queue: false,
        }];
    }
    let is_worker = |id: &ObjectId| t.views.iter().any(|v| v.id == *id && v.worker);
    let (workers, others): (Vec<_>, Vec<_>) = selected.iter().copied().partition(is_worker);

    let (work, target_pos): (Option<Command>, Vec2) =
        if let Some(node) = pick_node(t.nodes, world_pos, tolerance) {
            let cmd = Command::Gather {
                units: workers.clone(),
                node: node.id,
                queue: false,
            };
            (Some(cmd), node.pos)
        } else if let Some(b) =
            pick_building(t.buildings, world_pos).filter(|b| b.team == t.local && !b.finished())
        {
            let cmd = Command::Construct {
                units: workers.clone(),
                building: b.id,
                queue: false,
            };
            (Some(cmd), b.pos)
        } else {
            return vec![Command::Move {
                units: selected.to_vec(),
                target: to_fixed(world_pos),
                queue: false,
            }];
        };

    let mut cmds = Vec::new();
    if !workers.is_empty() {
        cmds.extend(work);
    }
    if !others.is_empty() {
        cmds.push(Command::Move {
            units: others,
            target: to_fixed(target_pos),
            queue: false,
        });
    }
    cmds
}

/// Right click with only buildings selected: set the rally point of those that train.
pub fn rally_command(
    buildings: &[BuildingView],
    selected: &[ObjectId],
    world_pos: Vec2,
) -> Option<Command> {
    let ids: Vec<ObjectId> = buildings
        .iter()
        .filter(|b| selected.contains(&b.id) && b.finished() && !b.trains().is_empty())
        .map(|b| b.id)
        .collect();
    (!ids.is_empty()).then(|| Command::SetRally {
        buildings: ids,
        target: to_fixed(world_pos),
    })
}

/// `Q`/`W`/`E`: queue the unit in `slot` of each selected, finished building's train list.
pub fn train_commands(
    buildings: &[BuildingView],
    selected: &[ObjectId],
    slot: usize,
) -> Vec<Command> {
    buildings
        .iter()
        .filter(|b| selected.contains(&b.id) && b.finished())
        .filter_map(|b| {
            Some(Command::Train {
                building: b.id,
                kind: *b.trains().get(slot)?,
            })
        })
        .collect()
}

/// Top-left cell for a footprint of `size` cells centered (as well as the grid allows) on `cursor`.
/// `None` when that is off the map to the top or left.
pub fn ghost_cell(cursor: Vec2, size: (u8, u8)) -> Option<Cell> {
    let cell = CELL_SIZE.to_f32();
    let cx = ((cursor.x - size.0 as f32 * cell / 2.0) / cell).round();
    let cy = ((cursor.y - size.1 as f32 * cell / 2.0) / cell).round();
    let ok = |v: f32| (0.0..=u16::MAX as f32).contains(&v);
    (ok(cx) && ok(cy)).then(|| Cell::new(cx as u16, cy as u16))
}

/// Center of a footprint in world units.
pub fn footprint_center(cell: Cell, size: (u8, u8)) -> Vec2 {
    let s = CELL_SIZE.to_f32();
    Vec2::new(
        (cell.x as f32 + size.0 as f32 / 2.0) * s,
        (cell.y as f32 + size.1 as f32 / 2.0) * s,
    )
}

/// Of the selected workers, the one closest to `site` (ties: lower id).
pub fn nearest_worker(views: &[UnitView], selected: &[ObjectId], site: Vec2) -> Option<ObjectId> {
    views
        .iter()
        .filter(|v| v.worker && selected.contains(&v.id))
        .min_by(|a, b| {
            a.pos
                .distance_squared(site)
                .total_cmp(&b.pos.distance_squared(site))
                .then(a.id.cmp(&b.id))
        })
        .map(|v| v.id)
}

/// Left click in build mode: the nearest selected worker goes and builds.
pub fn build_command(
    views: &[UnitView],
    selected: &[ObjectId],
    kind: BuildingKind,
    cell: Cell,
    queue: bool,
) -> Option<Command> {
    let site = footprint_center(cell, data::building(kind).size);
    Some(Command::Build {
        worker: nearest_worker(views, selected, site)?,
        kind,
        cell,
        queue,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::BuildingView;
    use dotwars_sim::data::UnitKind;

    const ME: PlayerId = PlayerId(0);

    fn unit(id: u32, worker: bool) -> UnitView {
        unit_at(id, worker, Vec2::ZERO)
    }

    fn unit_at(id: u32, worker: bool, pos: Vec2) -> UnitView {
        UnitView {
            id: ObjectId(id),
            team: ME,
            pos,
            radius: 8.0,
            worker,
            carrying: false,
            hp: (1, 1),
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

    fn building(
        id: u32,
        team: PlayerId,
        kind: BuildingKind,
        construction: Option<f32>,
    ) -> BuildingView {
        BuildingView {
            id: ObjectId(id),
            team,
            kind,
            pos: Vec2::new(500.0, 500.0),
            size: Vec2::new(96.0, 96.0),
            construction,
            queue: vec![UnitKind::Soldier],
            train_progress: 0.0,
            rally: None,
            hp: (1, 1),
        }
    }

    fn cmds(
        views: &[UnitView],
        nodes: &[NodeView],
        buildings: &[BuildingView],
        sel: &[u32],
        at: Vec2,
    ) -> Vec<Command> {
        let t = Targets {
            views,
            nodes,
            buildings,
            local: ME,
        };
        let sel: Vec<_> = sel.iter().map(|i| ObjectId(*i)).collect();
        right_click_commands(&t, &sel, at, 0.0)
    }

    #[test]
    fn click_on_a_node_splits_workers_from_the_rest() {
        let views = [unit(0, true), unit(1, false), unit(2, true)];
        let out = cmds(
            &views,
            &[node(10, 100.0)],
            &[],
            &[0, 1, 2],
            Vec2::new(105.0, 0.0),
        );
        assert_eq!(out.len(), 2);
        assert!(matches!(
            &out[0],
            Command::Gather { units, node, .. }
                if units == &[ObjectId(0), ObjectId(2)] && *node == ObjectId(10)
        ));
        assert!(matches!(&out[1], Command::Move { units, .. } if units == &[ObjectId(1)]));
    }

    #[test]
    fn click_on_ground_moves_everyone() {
        let views = [unit(0, true), unit(1, false)];
        let out = cmds(
            &views,
            &[node(10, 100.0)],
            &[],
            &[0, 1],
            Vec2::new(300.0, 0.0),
        );
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], Command::Move { units, .. } if units.len() == 2));
    }

    #[test]
    fn only_workers_selected_sends_just_gather() {
        let out = cmds(
            &[unit(0, true)],
            &[node(10, 100.0)],
            &[],
            &[0],
            Vec2::new(100.0, 0.0),
        );
        assert!(matches!(out.as_slice(), [Command::Gather { .. }]));
    }

    #[test]
    fn empty_selection_sends_nothing() {
        assert!(cmds(&[], &[node(10, 100.0)], &[], &[], Vec2::ZERO).is_empty());
    }

    #[test]
    fn unfinished_own_building_gets_constructed_by_workers() {
        let views = [unit(0, true), unit(1, false)];
        let site = [building(7, ME, BuildingKind::Barracks, Some(0.3))];
        let out = cmds(&views, &[], &site, &[0, 1], Vec2::new(510.0, 490.0));
        assert!(matches!(
            &out[0],
            Command::Construct { units, building, .. }
                if units == &[ObjectId(0)] && *building == ObjectId(7)
        ));
        assert!(matches!(&out[1], Command::Move { units, .. } if units == &[ObjectId(1)]));
    }

    #[test]
    fn finished_own_buildings_are_just_ground_and_enemy_ones_are_attacked() {
        let views = [unit(0, true)];
        let at = Vec2::new(510.0, 490.0);
        let own = building(7, ME, BuildingKind::Barracks, None);
        let out = cmds(&views, &[], &[own], &[0], at);
        assert!(matches!(out.as_slice(), [Command::Move { .. }]));
        let enemy = building(7, PlayerId(1), BuildingKind::Barracks, Some(0.3));
        let out = cmds(&views, &[], &[enemy], &[0], at);
        assert!(matches!(out.as_slice(), [Command::Attack { .. }]));
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

    #[test]
    fn rally_and_train_only_for_finished_producers() {
        let base = building(1, ME, BuildingKind::Base, None);
        let depot = building(2, ME, BuildingKind::SupplyDepot, None);
        let raw = building(3, ME, BuildingKind::Barracks, Some(0.5));
        let all = [base, depot, raw];
        let sel = [ObjectId(1), ObjectId(2), ObjectId(3)];
        let Some(Command::SetRally { buildings, .. }) =
            rally_command(&all, &sel, Vec2::new(1.0, 2.0))
        else {
            panic!("expected a rally command");
        };
        assert_eq!(buildings, [ObjectId(1)]);
        let train = train_commands(&all, &sel, 0);
        assert_eq!(train.len(), 1);
        assert!(matches!(
            &train[0],
            Command::Train { building, kind } if *building == ObjectId(1) && *kind == UnitKind::Worker
        ));
        assert!(rally_command(&all, &[], Vec2::ZERO).is_none());
    }

    #[test]
    fn ghost_is_centered_on_the_cursor_and_snaps() {
        // 2x2 depot: cursor on a cell corner puts the footprint around it.
        assert_eq!(
            ghost_cell(Vec2::new(64.0, 64.0), (2, 2)),
            Some(Cell::new(1, 1))
        );
        // 3x3: centered on a cell center.
        assert_eq!(
            ghost_cell(Vec2::new(112.0, 112.0), (3, 3)),
            Some(Cell::new(2, 2))
        );
        assert_eq!(ghost_cell(Vec2::new(-200.0, 50.0), (2, 2)), None);
        assert_eq!(
            footprint_center(Cell::new(2, 2), (3, 3)),
            Vec2::new(112.0, 112.0)
        );
    }

    #[test]
    fn the_nearest_selected_worker_builds() {
        let views = [
            unit_at(0, true, Vec2::new(900.0, 900.0)),
            unit_at(1, true, Vec2::new(110.0, 100.0)),
            unit_at(2, false, Vec2::new(112.0, 112.0)), // closer, but not a worker
            unit_at(3, true, Vec2::new(110.0, 100.0)),  // ties with 1: lower id
        ];
        let sel = [ObjectId(0), ObjectId(1), ObjectId(2), ObjectId(3)];
        let cmd = build_command(&views, &sel, BuildingKind::Barracks, Cell::new(2, 2), true);
        assert!(matches!(
            cmd,
            Some(Command::Build { worker, queue: true, .. }) if worker == ObjectId(1)
        ));
        assert!(
            build_command(
                &views,
                &[ObjectId(2)],
                BuildingKind::Base,
                Cell::new(0, 0),
                false
            )
            .is_none()
        );
    }

    fn enemy_at(id: u32, pos: Vec2) -> UnitView {
        UnitView {
            team: PlayerId(1),
            ..unit_at(id, false, pos)
        }
    }

    #[test]
    fn right_click_on_an_enemy_unit_attacks_with_everyone() {
        let views = [
            unit(1, true),
            unit(2, false),
            enemy_at(3, Vec2::new(50.0, 0.0)),
        ];
        let c = cmds(&views, &[], &[], &[1, 2], Vec2::new(52.0, 0.0));
        assert_eq!(
            c,
            [Command::Attack {
                units: vec![ObjectId(1), ObjectId(2)],
                target: ObjectId(3),
                queue: false
            }]
        );
    }

    #[test]
    fn right_click_on_an_enemy_building_attacks_it_and_own_units_are_just_ground() {
        let views = [unit(1, false), unit_at(2, false, Vec2::new(500.0, 500.0))];
        let enemy = building(9, PlayerId(1), BuildingKind::Barracks, None);
        let c = cmds(&views, &[], &[enemy], &[1], Vec2::new(510.0, 510.0));
        assert!(matches!(&c[..], [Command::Attack { target, .. }] if *target == ObjectId(9)));
        let c = cmds(&views, &[], &[], &[1], Vec2::new(500.0, 500.0));
        assert!(matches!(&c[..], [Command::Move { .. }]));
    }

    #[test]
    fn attack_mode_click_attacks_an_enemy_or_attack_moves_to_the_ground() {
        let views = [unit(1, false), enemy_at(3, Vec2::new(50.0, 0.0))];
        let t = Targets {
            views: &views,
            nodes: &[],
            buildings: &[],
            local: ME,
        };
        let sel = [ObjectId(1)];
        assert!(matches!(
            attack_click_command(&t, &sel, Vec2::new(50.0, 0.0), 0.0, false),
            Some(Command::Attack { .. })
        ));
        assert!(matches!(
            attack_click_command(&t, &sel, Vec2::new(300.0, 0.0), 0.0, true),
            Some(Command::AttackMove { queue: true, .. })
        ));
        assert!(attack_click_command(&t, &[], Vec2::ZERO, 0.0, false).is_none());
    }

    #[test]
    fn train_slots_pick_the_unit_by_position() {
        let barracks = building(1, ME, BuildingKind::Barracks, None);
        let sel = [ObjectId(1)];
        let kind = |slot| match &train_commands(std::slice::from_ref(&barracks), &sel, slot)[..] {
            [Command::Train { kind, .. }] => Some(*kind),
            _ => None,
        };
        assert_eq!(kind(0), Some(UnitKind::Soldier));
        assert_eq!(kind(1), Some(UnitKind::Ranger));
        assert_eq!(kind(2), Some(UnitKind::Brute));
        assert_eq!(kind(3), None);
    }
}
