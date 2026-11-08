mod controls;
mod effects;
mod groups;
mod orders;
mod selection;
mod view;

use controls::Action;
use dotwars_sim::{
    Command, DotWars,
    components::Worker,
    data::{self, BuildingKind, QUEUE_MAX},
};
use effects::{Effects, Kind};
use groups::ControlGroups;
use macroquad::prelude::*;
use phalanx_client::{
    Camera, RtsCameraController, backend,
    phalanx_input::{Gesture, InputState, MouseButton as Button, Rect},
};
use phalanx_core::PlayerId;
use phalanx_sim::{Driver, FixedStep};
use selection::{Selection, box_select, pick};
use view::{BarLevel, bar_level, show_bar};
use view::{
    BuildingView, NodeView, UnitView, build_buildings, build_dropoffs, build_nodes, build_views,
};

const INPUT_DELAY_TICKS: u32 = 3;
const LOCAL_PLAYER: PlayerId = PlayerId(0);
/// Placeholder until maps exist (docs/05-map.md).
const MAP_SIZE: Vec2 = Vec2::new(2048.0, 1536.0);
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
    let mut effects = Effects::default();

    let mut input = InputState::default();
    let bindings = controls::default_bindings();
    let mut selection = Selection::default();
    let mut groups = ControlGroups::default();
    let mut show_debug = true;
    let mut last_gesture: Option<Gesture> = None;
    // Client-only: which building the next left click places.
    let mut build_mode: Option<BuildingKind> = None;
    // Client-only: the next left click attacks (enemy) or attack-moves (ground).
    let mut attack_mode = false;

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
        let buildings = build_buildings(&driver.sim);
        let nodes = build_nodes(&driver.sim);

        handle_actions(
            &bindings,
            &input,
            &views,
            &buildings,
            &mut driver,
            &mut selection,
            &mut groups,
            &mut camera,
            &mut show_debug,
            &mut build_mode,
            &mut attack_mode,
            screen,
        );
        let world_mouse = camera.screen_to_world(input.mouse_pos().into());
        if let Some(kind) = build_mode {
            place_building(
                &input,
                &views,
                &selection,
                &camera,
                &mut driver,
                &mut build_mode,
                kind,
            );
        } else if attack_mode {
            let selected = selection.owned(&views, LOCAL_PLAYER);
            let targets = orders::Targets {
                views: &views,
                nodes: &nodes,
                buildings: &buildings,
                local: LOCAL_PLAYER,
            };
            for g in input.gestures() {
                let Gesture::Click {
                    button: Button::Left,
                    pos,
                    mods,
                } = *g
                else {
                    continue;
                };
                let world = camera.screen_to_world(pos.into());
                let tol = PICK_TOLERANCE_PX / camera.zoom;
                if let Some(cmd) =
                    orders::attack_click_command(&targets, &selected, world, tol, mods.shift)
                {
                    driver.issue(LOCAL_PLAYER, cmd);
                    attack_mode = mods.shift;
                }
            }
        } else {
            handle_gestures(
                &input,
                &views,
                &buildings,
                &camera,
                &mut selection,
                &mut last_gesture,
            );
        }

        if input.button_pressed(Button::Right) {
            if build_mode.is_some() || attack_mode {
                build_mode = None; // right click cancels, it does not also move
                attack_mode = false;
            } else {
                let selected = selection.owned(&views, LOCAL_PLAYER);
                let targets = orders::Targets {
                    views: &views,
                    nodes: &nodes,
                    buildings: &buildings,
                    local: LOCAL_PLAYER,
                };
                let mut cmds = orders::right_click_commands(
                    &targets,
                    &selected,
                    world_mouse,
                    PICK_TOLERANCE_PX / camera.zoom,
                );
                if selected.is_empty() {
                    let chosen = selection.owned_buildings(&buildings, LOCAL_PLAYER);
                    cmds.extend(orders::rally_command(&buildings, &chosen, world_mouse));
                }
                for cmd in cmds {
                    driver.issue(LOCAL_PLAYER, cmd);
                }
            }
        }

        // Sim at a fixed 20 Hz.
        let dt_micros = (get_frame_time() as f64 * 1_000_000.0) as u64;
        for _ in 0..step.advance(dt_micros) {
            driver.step(&mut events);
        }
        let dropoffs = build_dropoffs(&driver.sim);
        let buildings = build_buildings(&driver.sim);
        let views_now = build_views(&driver.sim, step.alpha());
        effects.on_events(&events, LOCAL_PLAYER, &views_now, &dropoffs, &buildings);
        events.clear();
        effects.update(get_frame_time());
        selection.prune(|id| driver.sim.entity(id).is_some());

        // Render: read-only, interpolated.
        let views = build_views(&driver.sim, step.alpha());
        let nodes = build_nodes(&driver.sim);
        clear_background(Color::from_rgba(20, 20, 28, 255));
        draw_grid(&camera, screen);
        draw_nodes(&nodes, &camera);
        draw_buildings(&buildings, &selection, &camera);
        draw_units(&views, &selection, &camera);
        if let Some(kind) = build_mode {
            draw_ghost(&driver.sim, kind, world_mouse, &camera);
        }
        draw_effects(&effects, &camera);
        if attack_mode {
            draw_text(
                "ATTACK: click an enemy or the ground",
                10.0,
                screen.y - 14.0,
                26.0,
                RED,
            );
        }
        draw_top_bar(&driver.sim, screen);
        draw_banner(&effects, screen);
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
    buildings: &[BuildingView],
    driver: &mut Driver<DotWars>,
    selection: &mut Selection,
    groups: &mut ControlGroups,
    camera: &mut Camera,
    show_debug: &mut bool,
    build_mode: &mut Option<BuildingKind>,
    attack_mode: &mut bool,
    screen: Vec2,
) {
    for action in bindings.triggered(input) {
        match action {
            Action::ToggleDebug => *show_debug = !*show_debug,
            Action::Build(kind) => {
                let has_worker = views
                    .iter()
                    .any(|v| v.worker && v.team == LOCAL_PLAYER && selection.contains(v.id));
                if has_worker {
                    *build_mode = Some(kind);
                }
            }
            Action::CancelMode => {
                *build_mode = None;
                *attack_mode = false;
            }
            Action::AttackMode => {
                if !selection.owned(views, LOCAL_PLAYER).is_empty() {
                    *attack_mode = true;
                    *build_mode = None;
                }
            }
            Action::Train(slot) => {
                let chosen = selection.owned_buildings(buildings, LOCAL_PLAYER);
                for cmd in orders::train_commands(buildings, &chosen, slot) {
                    driver.issue(LOCAL_PLAYER, cmd);
                }
            }
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
    buildings: &[BuildingView],
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
                match (picked, orders::pick_building(buildings, world)) {
                    // Units win over buildings. A building is selected alone.
                    (None, Some(b)) if !mods.shift => selection.replace([b.id]),
                    _ => selection.click(picked, mods.shift),
                }
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

/// Build mode: a left click sends the nearest selected worker to the footprint under the cursor.
fn place_building(
    input: &InputState,
    views: &[UnitView],
    selection: &Selection,
    camera: &Camera,
    driver: &mut Driver<DotWars>,
    build_mode: &mut Option<BuildingKind>,
    kind: BuildingKind,
) {
    for g in input.gestures() {
        let Gesture::Click {
            button: Button::Left,
            pos,
            mods,
        } = *g
        else {
            continue;
        };
        let world = camera.screen_to_world(pos.into());
        let Some(cell) = orders::ghost_cell(world, data::building(kind).size) else {
            continue;
        };
        let selected = selection.owned(views, LOCAL_PLAYER);
        if let Some(cmd) = orders::build_command(views, &selected, kind, cell, mods.shift) {
            driver.issue(LOCAL_PLAYER, cmd);
            if !mods.shift {
                *build_mode = None;
            }
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
        if v.carrying {
            draw_circle(p.x, p.y - r - 2.0 * camera.zoom, 3.0 * camera.zoom, GOLD);
        }
        let selected = selection.contains(v.id);
        if selected {
            let ring = if own { GREEN } else { YELLOW };
            draw_circle_lines(p.x, p.y, r + 3.0, 2.0, ring);
        }
        if show_bar(v.hp, selected) {
            draw_health_bar(p.x - r, p.y - r - 8.0 * camera.zoom.max(0.7), r * 2.0, v.hp);
        }
    }
}

fn draw_health_bar(x: f32, y: f32, w: f32, hp: (u32, u32)) {
    let color = match bar_level(hp) {
        BarLevel::Green => GREEN,
        BarLevel::Yellow => YELLOW,
        BarLevel::Red => RED,
    };
    let frac = hp.0 as f32 / hp.1.max(1) as f32;
    draw_rectangle(x, y, w, 4.0, Color::new(0.0, 0.0, 0.0, 0.7));
    draw_rectangle(x, y, w * frac, 4.0, color);
}

const GOLD_NODE: Color = Color::new(0.95, 0.78, 0.2, 1.0);

/// Nodes shrink as they run dry (radius ~ sqrt of what is left).
fn draw_nodes(nodes: &[NodeView], camera: &Camera) {
    for n in nodes {
        let p = camera.world_to_screen(n.pos);
        let left = (n.amount as f32 / dotwars_sim::economy::NODE_AMOUNT as f32).sqrt();
        let r = (n.radius * left).max(4.0) * camera.zoom;
        draw_circle(p.x, p.y, r, GOLD_NODE);
        draw_circle_lines(p.x, p.y, r, 1.0, Color::new(0.5, 0.4, 0.1, 1.0));
    }
}

fn team_color(team: PlayerId) -> Color {
    if team == LOCAL_PLAYER {
        Color::new(0.25, 0.55, 0.85, 1.0)
    } else {
        Color::new(0.8, 0.3, 0.3, 1.0)
    }
}

fn draw_buildings(buildings: &[BuildingView], selection: &Selection, camera: &Camera) {
    for b in buildings {
        let p = camera.world_to_screen(b.pos - b.size / 2.0);
        let (w, h) = (b.size.x * camera.zoom, b.size.y * camera.zoom);
        let mut color = team_color(b.team);
        if b.construction.is_some() {
            color = Color::new(color.r * 0.45, color.g * 0.45, color.b * 0.45, 1.0);
        }
        draw_rectangle(p.x, p.y, w, h, color);
        draw_rectangle_lines(p.x, p.y, w, h, 2.0, WHITE);
        draw_text(
            data::building(b.kind).name,
            p.x + 4.0,
            p.y + h / 2.0,
            18.0 * camera.zoom.max(0.7),
            WHITE,
        );
        if let Some(done) = b.construction {
            draw_rectangle(p.x, p.y + h + 3.0, w, 5.0, Color::new(0.0, 0.0, 0.0, 0.6));
            draw_rectangle(p.x, p.y + h + 3.0, w * done, 5.0, GREEN);
        }
        let selected = selection.contains(b.id);
        if show_bar(b.hp, selected) {
            draw_health_bar(p.x, p.y - 8.0, w, b.hp);
        }
        if !selected {
            continue;
        }
        let ring = if b.team == LOCAL_PLAYER {
            GREEN
        } else {
            YELLOW
        };
        draw_rectangle_lines(p.x - 3.0, p.y - 3.0, w + 6.0, h + 6.0, 2.0, ring);
        if let Some(front) = b.queue.first() {
            let text = format!(
                "{} {}/{}, {:.0}%",
                data::unit(*front).name,
                b.queue.len(),
                QUEUE_MAX,
                b.train_progress * 100.0
            );
            draw_text(&text, p.x, p.y - 6.0, 20.0, WHITE);
        }
        if let Some(r) = b.rally {
            let c = camera.world_to_screen(b.pos);
            let f = camera.world_to_screen(r);
            draw_line(c.x, c.y, f.x, f.y, 1.5, Color::new(0.3, 0.9, 0.4, 0.8));
            draw_triangle(
                vec2(f.x, f.y),
                vec2(f.x + 12.0, f.y - 5.0),
                vec2(f.x, f.y - 10.0),
                GREEN,
            );
        }
    }
}

/// Footprint under the cursor, green when `can_place` says yes and red otherwise.
fn draw_ghost(sim: &DotWars, kind: BuildingKind, cursor: Vec2, camera: &Camera) {
    let def = data::building(kind);
    let cell_size = dotwars_sim::CELL_SIZE.to_f32();
    let size = vec2(def.size.0 as f32, def.size.1 as f32) * cell_size;
    let (top_left, ok) = match orders::ghost_cell(cursor, def.size) {
        Some(c) => (
            vec2(c.x as f32, c.y as f32) * cell_size,
            sim.can_place(LOCAL_PLAYER, kind, c).is_ok(),
        ),
        None => (cursor - size / 2.0, false),
    };
    let p = camera.world_to_screen(top_left);
    let (w, h) = (size.x * camera.zoom, size.y * camera.zoom);
    let c = if ok { GREEN } else { RED };
    draw_rectangle(p.x, p.y, w, h, Color::new(c.r, c.g, c.b, 0.3));
    draw_rectangle_lines(p.x, p.y, w, h, 2.0, c);
}

fn draw_banner(effects: &Effects, screen: Vec2) {
    if let Some((text, age)) = effects.banner {
        let alpha = (1.0 - age / 2.0).clamp(0.0, 1.0);
        let dim = measure_text(text, None, 32, 1.0);
        draw_text(
            text,
            (screen.x - dim.width) / 2.0,
            screen.y / 2.0,
            32.0,
            Color::new(1.0, 0.3, 0.3, alpha),
        );
    }
}

fn draw_effects(effects: &Effects, camera: &Camera) {
    for e in &effects.list {
        let t = e.progress();
        let p = camera.world_to_screen(e.pos);
        match &e.kind {
            Kind::FloatText(text) => {
                let rise = 30.0 * t;
                draw_text(
                    text,
                    p.x - 10.0,
                    p.y - rise,
                    24.0,
                    Color::new(1.0, 0.9, 0.3, 1.0 - t),
                );
            }
            Kind::Flash(size) => {
                let q = camera.world_to_screen(e.pos - *size / 2.0);
                let (w, h) = (size.x * camera.zoom, size.y * camera.zoom);
                draw_rectangle(q.x, q.y, w, h, Color::new(1.0, 1.0, 1.0, 0.6 * (1.0 - t)));
            }
            Kind::Burst { radius, square } => {
                let r = (radius * (0.5 + t)) * camera.zoom;
                let c = Color::new(1.0, 0.6, 0.2, 0.8 * (1.0 - t));
                if *square {
                    draw_rectangle(p.x - r, p.y - r, r * 2.0, r * 2.0, c);
                } else {
                    draw_circle(p.x, p.y, r, c);
                }
            }
            Kind::Tracer(to) => {
                let q = camera.world_to_screen(*to);
                draw_line(p.x, p.y, q.x, q.y, 2.0, Color::new(1.0, 1.0, 0.6, 1.0 - t));
            }
            Kind::Hit => {
                let r = (4.0 + 8.0 * t) * camera.zoom;
                draw_circle_lines(p.x, p.y, r, 2.0, Color::new(1.0, 1.0, 1.0, 1.0 - t));
            }
            Kind::Ring => {
                let r = (8.0 + 24.0 * t) * camera.zoom;
                draw_circle_lines(p.x, p.y, r, 2.0, Color::new(0.95, 0.78, 0.2, 1.0 - t));
            }
        }
    }
}

fn draw_top_bar(sim: &DotWars, screen: Vec2) {
    let (used, cap) = sim.supply(LOCAL_PLAYER);
    let text = format!(
        "Minerals: {}   Supply: {used}/{cap}",
        sim.stock(LOCAL_PLAYER)
    );
    let color = if used > cap { RED } else { GOLD_NODE };
    draw_text(&text, screen.x / 2.0 - 140.0, 28.0, 30.0, color);
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
            "tick {}   RMB move/gather/attack/rally  S stop  H hold  A attack  B barracks  V depot  N base (workers)  Q/W/E train  Esc cancel  LMB click/box  Ctrl+N group  arrows/MMB pan  F1 debug",
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
        format!(
            "minerals {}   nodes {}   {}",
            driver.sim.stock(LOCAL_PLAYER),
            build_nodes(&driver.sim).len(),
            first_worker_state(driver, selection)
        ),
        first_combat_state(driver, selection),
    ];
    for (i, l) in lines.iter().enumerate() {
        draw_text(l, 10.0, 20.0 + i as f32 * 20.0, 20.0, WHITE);
    }
}

/// Gather state of the first selected worker, for the debug overlay.
fn first_worker_state(driver: &Driver<DotWars>, selection: &Selection) -> String {
    selection
        .ids()
        .find_map(|id| {
            let e = driver.sim.entity(id)?;
            let w = driver.sim.world.get::<&Worker>(e).ok()?;
            Some(format!("worker {}: carry {} {:?}", id.0, w.carry, w.state))
        })
        .unwrap_or_else(|| "no worker selected".into())
}

/// Target, cooldown and anchor of the first selected fighter, for the debug overlay.
fn first_combat_state(driver: &Driver<DotWars>, selection: &Selection) -> String {
    selection
        .ids()
        .find_map(|id| {
            let e = driver.sim.entity(id)?;
            let c = driver
                .sim
                .world
                .get::<&dotwars_sim::components::Combat>(e)
                .ok()?;
            let hp = driver
                .sim
                .world
                .get::<&dotwars_sim::components::Health>(e)
                .ok()?;
            Some(format!(
                "unit {}: hp {}/{}  target {:?}  cooldown {}  anchor {:?}",
                id.0,
                hp.hp,
                hp.max,
                c.target.map(|t| t.0),
                c.cooldown.remaining(),
                c.anchor.map(|a| a.to_f32()),
            ))
        })
        .unwrap_or_else(|| "no fighter selected".into())
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
