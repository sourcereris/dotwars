//! Selection: client-only state, never in the sim. Pure functions, no window needed.

use crate::view::UnitView;
use dotwars_sim::components::UnitId;
use macroquad::prelude::Vec2;
use phalanx_client::{Camera, phalanx_input::Rect};
use phalanx_core::PlayerId;
use std::collections::BTreeSet;

/// Sorted set, so the Commands built from it are identical on every run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection(BTreeSet<UnitId>);

impl Selection {
    pub fn contains(&self, id: UnitId) -> bool {
        self.0.contains(&id)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[cfg(test)]
    pub fn ids(&self) -> impl Iterator<Item = UnitId> + '_ {
        self.0.iter().copied()
    }

    pub fn replace(&mut self, ids: impl IntoIterator<Item = UnitId>) {
        self.0 = ids.into_iter().collect();
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Click result: `None` = empty ground.
    /// Without Shift the picked unit replaces the selection (ground clears it);
    /// with Shift it is toggled (ground changes nothing).
    pub fn click(&mut self, picked: Option<UnitId>, shift: bool) {
        match (picked, shift) {
            (Some(id), true) => {
                if !self.0.remove(&id) {
                    self.0.insert(id);
                }
            }
            (Some(id), false) => self.replace([id]),
            (None, true) => {}
            (None, false) => self.clear(),
        }
    }

    /// Box result. With Shift the units are added. Without it they replace the selection,
    /// but an empty box keeps the old one.
    pub fn box_select(&mut self, ids: Vec<UnitId>, shift: bool) {
        if shift {
            self.0.extend(ids);
        } else if !ids.is_empty() {
            self.replace(ids);
        }
    }

    /// Drops units that no longer exist.
    pub fn prune(&mut self, alive: impl Fn(UnitId) -> bool) {
        self.0.retain(|id| alive(*id));
    }

    /// Selected units that belong to `local`: the ones that may receive commands.
    pub fn owned(&self, views: &[UnitView], local: PlayerId) -> Vec<UnitId> {
        views
            .iter()
            .filter(|v| v.team == local && self.0.contains(&v.id))
            .map(|v| v.id)
            .collect()
    }
}

/// Nearest unit whose body (plus `tolerance` world units) covers `world_pos`.
/// Ties go to the lower `UnitId`.
pub fn pick(views: &[UnitView], world_pos: Vec2, tolerance: f32) -> Option<UnitId> {
    let mut best: Option<(f32, UnitId)> = None;
    for v in views {
        let d = v.pos.distance(world_pos);
        if d <= v.radius + tolerance && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, v.id));
        }
    }
    best.map(|(_, id)| id)
}

/// Own units whose centre is inside the screen-space rectangle.
pub fn box_select(views: &[UnitView], rect: Rect, camera: &Camera, local: PlayerId) -> Vec<UnitId> {
    views
        .iter()
        .filter(|v| v.team == local)
        .filter(|v| {
            let s = camera.world_to_screen(v.pos);
            rect.contains([s.x, s.y])
        })
        .map(|v| v.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: PlayerId = PlayerId(0);
    const FOE: PlayerId = PlayerId(1);

    fn view(id: u32, team: PlayerId, x: f32, y: f32) -> UnitView {
        UnitView {
            id: UnitId(id),
            team,
            pos: Vec2::new(x, y),
            radius: 8.0,
        }
    }

    fn views() -> Vec<UnitView> {
        vec![
            view(0, ME, 100.0, 100.0),
            view(1, ME, 112.0, 100.0), // overlaps unit 0
            view(2, FOE, 300.0, 100.0),
        ]
    }

    #[test]
    fn pick_takes_the_nearest_of_overlapping_units() {
        let v = views();
        assert_eq!(pick(&v, Vec2::new(109.0, 100.0), 0.0), Some(UnitId(1)));
        assert_eq!(pick(&v, Vec2::new(103.0, 100.0), 0.0), Some(UnitId(0)));
    }

    #[test]
    fn pick_tie_goes_to_lower_id() {
        let v = views();
        assert_eq!(pick(&v, Vec2::new(106.0, 100.0), 0.0), Some(UnitId(0)));
    }

    #[test]
    fn pick_misses_outside_radius_plus_tolerance() {
        let v = views();
        let p = Vec2::new(300.0, 112.0);
        assert_eq!(pick(&v, p, 0.0), None); // 12 away, body reaches 8
    }

    #[test]
    fn pick_tolerance_extends_reach() {
        let v = views();
        assert_eq!(pick(&v, Vec2::new(300.0, 111.0), 4.0), Some(UnitId(2)));
        assert_eq!(pick(&v, Vec2::new(300.0, 113.0), 4.0), None);
    }

    #[test]
    fn box_ignores_enemies_and_respects_camera() {
        let v = views();
        let cam = Camera::default();
        let all = Rect::from_corners([0.0, 0.0], [500.0, 500.0]);
        assert_eq!(box_select(&v, all, &cam, ME), [UnitId(0), UnitId(1)]);

        // Zoomed in 2x with the view moved: unit 0 is at screen (0, 0) .. unit 1 at (24, 0).
        let cam = Camera {
            offset: Vec2::new(100.0, 100.0),
            zoom: 2.0,
        };
        let small = Rect::from_corners([-5.0, -5.0], [10.0, 10.0]);
        assert_eq!(box_select(&v, small, &cam, ME), [UnitId(0)]);
    }

    #[test]
    fn click_replaces_and_clears() {
        let mut s = Selection::default();
        s.click(Some(UnitId(0)), false);
        s.click(Some(UnitId(1)), false);
        assert_eq!(s.ids().collect::<Vec<_>>(), [UnitId(1)]);
        s.click(None, true);
        assert_eq!(s.len(), 1);
        s.click(None, false);
        assert!(s.is_empty());
    }

    #[test]
    fn shift_click_toggles() {
        let mut s = Selection::default();
        s.click(Some(UnitId(0)), true);
        s.click(Some(UnitId(1)), true);
        assert_eq!(s.len(), 2);
        s.click(Some(UnitId(0)), true);
        assert_eq!(s.ids().collect::<Vec<_>>(), [UnitId(1)]);
    }

    #[test]
    fn box_replaces_adds_or_keeps() {
        let mut s = Selection::default();
        s.box_select(vec![UnitId(0)], false);
        s.box_select(vec![], false); // empty box keeps the selection
        assert_eq!(s.ids().collect::<Vec<_>>(), [UnitId(0)]);
        s.box_select(vec![UnitId(1)], true);
        assert_eq!(s.len(), 2);
        s.box_select(vec![UnitId(5)], false);
        assert_eq!(s.ids().collect::<Vec<_>>(), [UnitId(5)]);
    }

    #[test]
    fn prune_and_owned() {
        let v = views();
        let mut s = Selection::default();
        s.replace([UnitId(0), UnitId(2), UnitId(9)]);
        s.prune(|id| id.0 != 9);
        assert_eq!(s.len(), 2);
        // Enemy unit 2 is selected (inspecting) but cannot receive commands.
        assert_eq!(s.owned(&v, ME), [UnitId(0)]);
    }
}
