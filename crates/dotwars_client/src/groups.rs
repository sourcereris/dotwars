//! Control groups (Ctrl+N assigns, N selects, double tap centers). Client-only state.
//! Time is passed in, so the logic is a pure function of its inputs.

use dotwars_sim::components::ObjectId;
use std::collections::BTreeSet;

/// Two taps on the same number within this many seconds also center the camera.
pub const DOUBLE_TAP_SECONDS: f64 = 0.3;

#[derive(Clone, Debug, Default)]
pub struct ControlGroups {
    groups: [BTreeSet<ObjectId>; 10],
    last_tap: Option<(u8, f64)>,
}

/// Result of pressing a group number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupSelect {
    pub ids: Vec<ObjectId>,
    pub center: bool,
}

impl ControlGroups {
    /// Replaces group `n`. An empty selection is ignored, so a stray Ctrl+N can't wipe a group.
    pub fn assign(&mut self, n: u8, ids: impl IntoIterator<Item = ObjectId>) {
        let set: BTreeSet<ObjectId> = ids.into_iter().collect();
        if !set.is_empty() {
            self.groups[n as usize % 10] = set;
        }
    }

    pub fn add(&mut self, n: u8, ids: impl IntoIterator<Item = ObjectId>) {
        self.groups[n as usize % 10].extend(ids);
    }

    #[cfg(test)]
    pub fn ids(&self, n: u8) -> impl Iterator<Item = ObjectId> + '_ {
        self.groups[n as usize % 10].iter().copied()
    }

    /// Dead units are dropped first. `None` when the group is empty (selection stays as it is).
    pub fn select(
        &mut self,
        n: u8,
        now: f64,
        alive: impl Fn(ObjectId) -> bool,
    ) -> Option<GroupSelect> {
        let n = n % 10;
        let group = &mut self.groups[n as usize];
        group.retain(|id| alive(*id));
        if group.is_empty() {
            self.last_tap = None;
            return None;
        }
        let center =
            matches!(self.last_tap, Some((m, t)) if m == n && now - t <= DOUBLE_TAP_SECONDS);
        // A third quick tap starts a new pair.
        self.last_tap = if center { None } else { Some((n, now)) };
        Some(GroupSelect {
            ids: group.iter().copied().collect(),
            center,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[u32]) -> Vec<ObjectId> {
        v.iter().map(|i| ObjectId(*i)).collect()
    }

    fn alive(_: ObjectId) -> bool {
        true
    }

    #[test]
    fn assign_select_add() {
        let mut g = ControlGroups::default();
        g.assign(1, ids(&[3, 1]));
        let s = g.select(1, 0.0, alive).unwrap();
        assert_eq!(s.ids, ids(&[1, 3]));
        assert!(!s.center);
        g.add(1, ids(&[2]));
        assert_eq!(g.select(1, 5.0, alive).unwrap().ids, ids(&[1, 2, 3]));
    }

    #[test]
    fn assign_replaces_but_ignores_empty() {
        let mut g = ControlGroups::default();
        g.assign(2, ids(&[1, 2]));
        g.assign(2, ids(&[7]));
        assert_eq!(g.ids(2).collect::<Vec<_>>(), ids(&[7]));
        g.assign(2, ids(&[]));
        assert_eq!(g.ids(2).collect::<Vec<_>>(), ids(&[7]));
    }

    #[test]
    fn dead_units_are_pruned() {
        let mut g = ControlGroups::default();
        g.assign(0, ids(&[1, 2, 3]));
        let s = g.select(0, 0.0, |id| id.0 != 2).unwrap();
        assert_eq!(s.ids, ids(&[1, 3]));
        assert_eq!(g.select(0, 9.0, |_| false), None);
    }

    #[test]
    fn empty_group_selects_nothing() {
        let mut g = ControlGroups::default();
        assert_eq!(g.select(4, 0.0, alive), None);
    }

    #[test]
    fn double_tap_centers_only_inside_the_window_and_on_the_same_number() {
        let mut g = ControlGroups::default();
        g.assign(1, ids(&[1]));
        g.assign(2, ids(&[2]));

        assert!(!g.select(1, 0.0, alive).unwrap().center);
        assert!(g.select(1, 0.2, alive).unwrap().center);
        // Third tap starts a new pair.
        assert!(!g.select(1, 0.3, alive).unwrap().center);

        // Too slow.
        assert!(!g.select(1, 5.0, alive).unwrap().center);
        assert!(!g.select(1, 5.5, alive).unwrap().center);
        // Different number.
        assert!(!g.select(2, 5.6, alive).unwrap().center);
        assert!(!g.select(1, 5.7, alive).unwrap().center);
    }
}
