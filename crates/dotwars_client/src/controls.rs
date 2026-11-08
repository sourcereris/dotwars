//! Dot Wars actions and their default keys. The engine knows keys; this file knows what they mean.

use phalanx_client::phalanx_input::{Bindings, Chord, InputState, Key};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleDebug,
    Stop,
    Hold,
    PanUp,
    PanDown,
    PanLeft,
    PanRight,
    /// `N`: select group N (double tap also centers the camera).
    GroupSelect(u8),
    /// `Ctrl+N`: assign the selection to group N.
    GroupAssign(u8),
    /// `Shift+N`: add the selection to group N.
    GroupAdd(u8),
}

const DIGITS: [Key; 10] = [
    Key::Num0,
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

pub fn default_bindings() -> Bindings<Action> {
    let mut b = Bindings::default();
    b.bind(Key::F1, Action::ToggleDebug)
        .bind(Key::S, Action::Stop)
        .bind(Key::H, Action::Hold)
        // Arrows only: WASD would collide with S = Stop.
        .bind(Key::Up, Action::PanUp)
        .bind(Key::Down, Action::PanDown)
        .bind(Key::Left, Action::PanLeft)
        .bind(Key::Right, Action::PanRight);
    for (n, key) in DIGITS.iter().enumerate() {
        let n = n as u8;
        b.bind(*key, Action::GroupSelect(n))
            .bind(Chord::key(*key).ctrl(), Action::GroupAssign(n))
            .bind(Chord::key(*key).shift(), Action::GroupAdd(n));
    }
    b
}

/// Held pan keys as a direction, each axis -1, 0 or 1.
pub fn pan_dir(b: &Bindings<Action>, input: &InputState) -> [f32; 2] {
    let axis = |neg, pos| b.is_held(input, &pos) as i8 as f32 - b.is_held(input, &neg) as i8 as f32;
    [
        axis(Action::PanLeft, Action::PanRight),
        axis(Action::PanUp, Action::PanDown),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use phalanx_client::phalanx_input::InputEvent;

    fn frame(s: &mut InputState, ev: &[InputEvent]) {
        s.begin_frame();
        for e in ev {
            s.handle(*e);
        }
    }

    #[test]
    fn digit_chords_are_distinct() {
        let b = default_bindings();
        let mut s = InputState::default();
        frame(&mut s, &[InputEvent::KeyDown(Key::Num3)]);
        assert_eq!(
            b.triggered(&s).collect::<Vec<_>>(),
            [Action::GroupSelect(3)]
        );
        frame(
            &mut s,
            &[
                InputEvent::KeyUp(Key::Num3),
                InputEvent::KeyDown(Key::LeftCtrl),
                InputEvent::KeyDown(Key::Num3),
            ],
        );
        assert_eq!(
            b.triggered(&s).collect::<Vec<_>>(),
            [Action::GroupAssign(3)]
        );
    }

    #[test]
    fn pan_dir_from_arrows() {
        let b = default_bindings();
        let mut s = InputState::default();
        frame(
            &mut s,
            &[
                InputEvent::KeyDown(Key::Right),
                InputEvent::KeyDown(Key::Up),
            ],
        );
        assert_eq!(pan_dir(&b, &s), [1.0, -1.0]);
        frame(
            &mut s,
            &[InputEvent::KeyDown(Key::Left)], // left + right cancel
        );
        assert_eq!(pan_dir(&b, &s), [0.0, -1.0]);
    }
}
