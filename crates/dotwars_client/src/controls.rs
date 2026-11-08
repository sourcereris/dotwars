//! Dot Wars actions and their default keys. The engine knows keys; this file knows what they mean.

use phalanx_client::phalanx_input::{Bindings, Key};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleDebug,
    // C2+: Stop, Hold, SelectGroup(u8), AssignGroup(u8), ...
}

pub fn default_bindings() -> Bindings<Action> {
    let mut b = Bindings::default();
    b.bind(Key::F1, Action::ToggleDebug);
    b
}
