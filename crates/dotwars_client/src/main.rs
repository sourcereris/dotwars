use dotwars_sim::{
    Command, DotWars,
    components::{Position, PrevPosition, UnitId},
};
use macroquad::prelude::*;
use phalanx_client::{Camera, lerp_pos, to_fixed};
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

    loop {
        // Input -> Commands.
        if is_mouse_button_pressed(MouseButton::Right) {
            let target = to_fixed(camera.screen_to_world(mouse_position().into()));
            driver.issue(
                LOCAL_PLAYER,
                Command::Move {
                    units: vec![unit],
                    target,
                },
            );
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
        draw_text(
            format!("tick {}   right click = move", driver.tick()),
            10.0,
            20.0,
            20.0,
            WHITE,
        );
        next_frame().await;
    }
}
