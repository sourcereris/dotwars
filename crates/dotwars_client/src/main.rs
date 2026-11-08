mod controls;
mod groups;
mod selection;
mod view;

use controls::Action;
use dotwars_sim::{Command, DotWars};
use groups::ControlGroups;
use macroquad::prelude::*;
use phalanx_client::{
    Camera, RtsCameraController, backend,
    phalanx_input::{Gesture, InputState, MouseButton as Button, Rect},
    to_fixed,
};
use phalanx_core::PlayerId;
use phalanx_sim::{Driver, FixedStep};
use selection::{Selection, box_select, pick};
use view::{UnitView, build_views};

const INPUT_DELAY_TICKS: u32 = 3;
const LOCAL_PLAYER: PlayerId = PlayerId(0);
/// Placeholder until maps exist (docs/05-map.md).
const MAP_SIZE: Vec2 = Vec2::new(2000.0, 1500.0);
/// Click reach beyond a unit's body, in screen pixels.
const PICK_TOLERANCE_PX: f32 = 4.0;

#[macroquad::main("Dot Wars")]
async fn main() {
    let mut driver = Driver::new(DotWars::demo_scenario(1), INPUT_DELAY_TICKS);
    let mut step = FixedStep::default();
    let mut camera = Camera::default();
    let controller = RtsCameraController {
        bounds: Some(Rect::from_corners([0.0, 0.0], MAP_SIZE.into())),
        ..Default::default()
    };
    let mut events = Vec::new();

    let mut input = InputState::default();
    let bindings = controls::default_bindings();
    let mut selection = Selection::default();
    let mut groups = ControlGroups::default();
    let mut show_debug = true;
    let mut last_gesture: Option<Gesture> = None;

    loop {
        let screen = vec2(screen_width(), screen_height());

        // Input. No macroquad input types past this line.
        backend::mq::poll(&mut input);
        controller.update(
            &mut camera,
            &input,
            controls::pan_dir(&bindings, &input),
            screen,
            get_frame_time(),
        );

        // Picking works on what is on screen: interpolated positions at the last drawn alpha.
        let views = build_views(&driver.sim, step.alpha());

        handle_actions(
            &bindings,
            &input,
            &views,
            &mut driver,
            &mut selection,
            &mut groups,
            &mut camera,
            &mut show_debug,
            screen,
        );
        handle_gestures(&input, &views, &camera, &mut selection, &mut last_gesture);

        if input.button_pressed(Button::Right) {
            let units = selection.owned(&views, LOCAL_PLAYER);
            if !units.is_empty() {
                let target = to_fixed(camera.screen_to_world(input.mouse_pos().into()));
                driver.issue(
                    LOCAL_PLAYER,
                    Command::Move {
                        units,
                        target,
                        queue: false,
                    },
                );
            }
        }

        // Sim at a fixed 20 Hz.
        let dt_micros = (get_frame_time() as f64 * 1_000_000.0) as u64;
        for _ in 0..step.advance(dt_micros) {
            driver.step(&mut events);
            events.clear(); // TODO: sound / effects / UI
        }
        selection.prune(|id| driver.sim.entity(id).is_some());

        // Render: read-only, interpolated.
        let views = build_views(&driver.sim, step.alpha());
        clear_background(Color::from_rgba(20, 20, 28, 255));
        draw_grid(&camera, screen);
        draw_units(&views, &selection, &camera);
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
            draw_debug(&driver, &input, &selection, &camera, last_gesture);
        }
        next_frame().await;
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_actions(
    bindings: &phalanx_client::phalanx_input::Bindings<Action>,
    input: &InputState,
    views: &[UnitView],
    driver: &mut Driver<DotWars>,
    selection: &mut Selection,
    groups: &mut ControlGroups,
    camera: &mut Camera,
    show_debug: &mut bool,
    screen: Vec2,
) {
    for action in bindings.triggered(input) {
        match action {
            Action::ToggleDebug => *show_debug = !*show_debug,
            Action::Stop => {
                let units = selection.owned(views, LOCAL_PLAYER);
                if !units.is_empty() {
                    driver.issue(LOCAL_PLAYER, Command::Stop { units });
                }
            }
            Action::Hold => {
                let units = selection.owned(views, LOCAL_PLAYER);
                if !units.is_empty() {
                    driver.issue(LOCAL_PLAYER, Command::Hold { units });
                }
            }
            Action::GroupAssign(n) => groups.assign(n, selection.owned(views, LOCAL_PLAYER)),
            Action::GroupAdd(n) => groups.add(n, selection.owned(views, LOCAL_PLAYER)),
            Action::GroupSelect(n) => {
                let alive = |id| driver.sim.entity(id).is_some();
                if let Some(sel) = groups.select(n, get_time(), alive) {
                    if sel.center {
                        let pts: Vec<Vec2> = views
                            .iter()
                            .filter(|v| sel.ids.contains(&v.id))
                            .map(|v| v.pos)
                            .collect();
                        if !pts.is_empty() {
                            let centroid = pts.iter().sum::<Vec2>() / pts.len() as f32;
                            camera.center_on(centroid, screen);
                        }
                    }
                    selection.replace(sel.ids);
                }
            }
            // Pan actions are read as held keys by `controls::pan_dir`.
            Action::PanUp | Action::PanDown | Action::PanLeft | Action::PanRight => {}
        }
    }
}

fn handle_gestures(
    input: &InputState,
    views: &[UnitView],
    camera: &Camera,
    selection: &mut Selection,
    last_gesture: &mut Option<Gesture>,
) {
    for g in input.gestures() {
        match *g {
            Gesture::Click {
                button: Button::Left,
                pos,
                mods,
            } => {
                let world = camera.screen_to_world(pos.into());
                let picked = pick(views, world, PICK_TOLERANCE_PX / camera.zoom);
                selection.click(picked, mods.shift);
                *last_gesture = Some(*g);
            }
            Gesture::DragEnd {
                button: Button::Left,
                rect,
                mods,
            } => {
                selection.box_select(box_select(views, rect, camera, LOCAL_PLAYER), mods.shift);
                *last_gesture = Some(*g);
            }
            _ => {}
        }
    }
}

fn draw_grid(camera: &Camera, screen: Vec2) {
    let step = 100.0;
    let line = Color::new(1.0, 1.0, 1.0, 0.06);
    let edge = Color::new(1.0, 1.0, 1.0, 0.35);
    let mut x = 0.0;
    while x <= MAP_SIZE.x {
        let a = camera.world_to_screen(vec2(x, 0.0));
        let b = camera.world_to_screen(vec2(x, MAP_SIZE.y));
        if a.x >= -1.0 && a.x <= screen.x + 1.0 {
            let c = if x == 0.0 || x == MAP_SIZE.x {
                edge
            } else {
                line
            };
            draw_line(a.x, a.y, b.x, b.y, 1.0, c);
        }
        x += step;
    }
    let mut y = 0.0;
    while y <= MAP_SIZE.y {
        let a = camera.world_to_screen(vec2(0.0, y));
        let b = camera.world_to_screen(vec2(MAP_SIZE.x, y));
        if a.y >= -1.0 && a.y <= screen.y + 1.0 {
            let c = if y == 0.0 || y == MAP_SIZE.y {
                edge
            } else {
                line
            };
            draw_line(a.x, a.y, b.x, b.y, 1.0, c);
        }
        y += step;
    }
}

fn draw_units(views: &[UnitView], selection: &Selection, camera: &Camera) {
    for v in views {
        let p = camera.world_to_screen(v.pos);
        let r = v.radius * camera.zoom;
        let own = v.team == LOCAL_PLAYER;
        let fill = if own {
            SKYBLUE
        } else {
            Color::from_rgba(230, 90, 90, 255)
        };
        draw_circle(p.x, p.y, r, fill);
        if selection.contains(v.id) {
            let ring = if own { GREEN } else { YELLOW };
            draw_circle_lines(p.x, p.y, r + 3.0, 2.0, ring);
        }
    }
}

fn draw_debug(
    driver: &Driver<DotWars>,
    input: &InputState,
    selection: &Selection,
    camera: &Camera,
    last_gesture: Option<Gesture>,
) {
    let m = input.mouse_pos();
    let mods = input.modifiers();
    let lines = [
        format!(
            "tick {}   RMB move  S stop  H hold  LMB click/box (Shift adds)  Ctrl+N group  arrows/edges/MMB pan  wheel zoom  F1 debug",
            driver.tick()
        ),
        format!(
            "mouse {:.0},{:.0}   mods {}{}{}   zoom {:.2}   cam {:.0},{:.0}",
            m[0],
            m[1],
            if mods.shift { "S" } else { "-" },
            if mods.ctrl { "C" } else { "-" },
            if mods.alt { "A" } else { "-" },
            camera.zoom,
            camera.offset.x,
            camera.offset.y,
        ),
        format!(
            "selected {}   last left gesture: {}",
            selection.len(),
            describe(last_gesture)
        ),
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
