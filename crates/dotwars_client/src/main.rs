mod controls;

use controls::Action;
use dotwars_sim::{
    Command, DotWars,
    components::{Position, PrevPosition, UnitId},
};
use macroquad::prelude::*;
use phalanx_client::{
    Camera, backend, lerp_pos,
    phalanx_input::{Gesture, InputState, MouseButton as Button},
    to_fixed,
};
use phalanx_core::{FixedVec2, PlayerId};
use phalanx_sim::{Driver, FixedStep};

const INPUT_DELAY_TICKS: u32 = 3;
const LOCAL_PLAYER: PlayerId = PlayerId(0);

#[macroquad::main("Dot Wars")]
async fn main() {
    let mut sim = DotWars::new(1);
    let unit = sim.spawn_unit(FixedVec2::from_ints(100, 100));
    let mut driver = Driver::new(sim, INPUT_DELAY_TICKS);
    let mut step = FixedStep::default();
    let camera = Camera::default();
    let mut events = Vec::new();

    let mut input = InputState::default();
    let bindings = controls::default_bindings();
    let mut show_debug = true;
    let mut last_gesture: Option<Gesture> = None;

    loop {
        // Input → actions and Commands. No macroquad input types past this line.
        backend::mq::poll(&mut input);

        for action in bindings.triggered(&input) {
            match action {
                Action::ToggleDebug => show_debug = !show_debug,
            }
        }

        // Move on press, not release: RTS orders should feel instant.
        if input.button_pressed(Button::Right) {
            let target = to_fixed(camera.screen_to_world(input.mouse_pos().into()));
            driver.issue(
                LOCAL_PLAYER,
                Command::Move {
                    units: vec![unit],
                    target,
                },
            );
        }

        // Left click / box: selection arrives in C2. For now just remember the gesture.
        if let Some(g) = input.gestures().iter().find(|g| {
            matches!(
                g,
                Gesture::Click {
                    button: Button::Left,
                    ..
                } | Gesture::DragEnd {
                    button: Button::Left,
                    ..
                }
            )
        }) {
            last_gesture = Some(*g);
        }

        // Sim at a fixed 20 Hz.
        let dt_micros = (get_frame_time() as f64 * 1_000_000.0) as u64;
        for _ in 0..step.advance(dt_micros) {
            driver.step(&mut events);
            events.clear(); // TODO: sound / effects / UI
        }

        // Render: read-only, interpolated.
        clear_background(Color::from_rgba(20, 20, 28, 255));
        let alpha = step.alpha();
        for (_, (_id, prev, pos)) in driver
            .sim
            .world
            .query::<(&UnitId, &PrevPosition, &Position)>()
            .iter()
        {
            let p = camera.world_to_screen(lerp_pos(prev.0, pos.0, alpha));
            draw_rectangle(p.x - 6.0, p.y - 6.0, 12.0, 12.0, SKYBLUE);
        }

        if let Some(r) = input.drag_rect(Button::Left) {
            let [w, h] = r.size();
            draw_rectangle(r.min[0], r.min[1], w, h, Color::new(0.3, 0.9, 0.4, 0.12));
            draw_rectangle_lines(
                r.min[0],
                r.min[1],
                w,
                h,
                1.0,
                Color::new(0.3, 0.9, 0.4, 0.9),
            );
        }

        if show_debug {
            draw_debug(&driver, &input, last_gesture);
        }
        next_frame().await;
    }
}

fn draw_debug(driver: &Driver<DotWars>, input: &InputState, last_gesture: Option<Gesture>) {
    let m = input.mouse_pos();
    let mods = input.modifiers();
    let lines = [
        format!(
            "tick {}   RMB = move   LMB drag = box   F1 = debug",
            driver.tick()
        ),
        format!(
            "mouse {:.0},{:.0}   mods {}{}{}",
            m[0],
            m[1],
            if mods.shift { "S" } else { "-" },
            if mods.ctrl { "C" } else { "-" },
            if mods.alt { "A" } else { "-" },
        ),
        format!("last left gesture: {}", describe(last_gesture)),
    ];
    for (i, l) in lines.iter().enumerate() {
        draw_text(l, 10.0, 20.0 + i as f32 * 20.0, 20.0, WHITE);
    }
}

fn describe(g: Option<Gesture>) -> String {
    match g {
        None => "-".into(),
        Some(Gesture::Click { pos, .. }) => format!("click at {:.0},{:.0}", pos[0], pos[1]),
        Some(Gesture::DragEnd { rect, .. }) => {
            let [w, h] = rect.size();
            format!("box {w:.0}x{h:.0} at {:.0},{:.0}", rect.min[0], rect.min[1])
        }
    }
}
