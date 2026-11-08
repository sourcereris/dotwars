# 01 – Control

Feature list: [GAME_DESIGN.md § Control](../GAME_DESIGN.md#1-control) · Engine input: [phalanx/docs/input.md](../../phalanx/docs/input.md)

## Design

### Layers

```
raw input ──► InputState ──► Bindings ──► Controller ──► Command ──► Driver ──► sim
 (engine)      (engine)      (engine)       (game)        (game)     (engine)
                                              │
                                              └──► selection, groups, feedback (client only)
```

**Key rule:** selection and control groups are **client-only state**. They are not in the sim,
not sent over the network and need not be deterministic. The sim only gets the final Command
with a list of `ObjectId`s.

### Selection (client)
- **Click:** nearest own-or-visible unit within a radius of the mouse, compared in world space
  using *interpolated* positions (you click what you see).
- **Box:** project units to the screen and test against the drag rectangle. Works unchanged in 2.5D.
- **Priority:** if the box contains own units, ignore enemies and buildings. Units beat buildings.
- **Shift** adds/removes; **Ctrl+click** (later) selects all of that type on screen.
- Dead units are pruned from the selection every frame.
- Written as pure functions (`pick`, `box_select`) with no macroquad, so they're unit-testable.

### Smart right click (client)
The client decides what a right click means; the sim gets an explicit command and never guesses:

| Clicked on | Selection | Command |
|---|---|---|
| enemy unit/building | any | `Attack` |
| resource | has workers | `Gather` (economy) |
| ground | any | `Move` |
| ground + attack-move key | any | `AttackMove` |

### Commands (sim)
```rust
enum Command {
    Move       { units: Vec<ObjectId>, target: FixedVec2, queue: bool },
    AttackMove { units: Vec<ObjectId>, target: FixedVec2, queue: bool },
    Attack     { units: Vec<ObjectId>, target: ObjectId,    queue: bool },
    Stop       { units: Vec<ObjectId> },
    Hold       { units: Vec<ObjectId> },
}
```
- **Ownership check:** the sim ignores units that don't belong to the issuing player (guards against bugs and cheats). Needs a `Team` component.
- Units that died between issue and execution are skipped.

### Orders (sim)
`Orders` component = current order + queue (`Vec<Order>`), replacing today's `MoveTarget`.
- `queue: false` clears the queue and replaces the current order; `queue: true` appends.
- Systems read only the current order; when it completes, the next one starts.
- `Order::{Move(pos), AttackMove(pos), Attack(ObjectId), Hold}`. No order = idle.

### Group movement (sim, deterministic)
20 units sent to one point would fight over one pixel. MVP: keep the group's shape.
Each unit targets `target + (own_pos − group_center)`, with the offset clamped to a maximum radius.
Computed in the sim from the command, so commands stay small. Proper formations: later.

### Control groups (client)
`[Vec<ObjectId>; 10]`. Ctrl+N assigns, N selects, double-tap N centers the camera. Dead units pruned.

### Feedback (client)
On click, show a marker **immediately** (green = move, red = attack), sound later. Hides the
150 ms lockstep input delay.

### Camera (engine)
Edge scroll, WASD/arrows, middle-mouse drag, wheel zoom. 2D now, 2.5D later.

### Testing
- Sim: orders, queue, ownership check, group offsets: headless, like the existing determinism test.
- Client: selection and smart-command logic as pure functions with unit tests.

## Steps

| Step | What | Where | Status |
|---|---|---|---|
| C0 | Single unit, hard-coded Move on right click, interpolation | game | ✅ |
| C1 | `phalanx_input` (InputState, drag detection, Bindings) + macroquad adapter; drag rect drawn | engine | ✅ |
| C2 | Many units, `Team`, click/box/Shift selection, selection circles | game | ✅ |
| C3 | `Orders` replaces `MoveTarget`; Move/Stop/Hold; ownership check | game sim | ✅ |
| C4 | Group movement keeping formation shape | game sim | ⬜ |
| C5 | Camera pan/zoom + control groups | engine + game | ✅ |
| C6 | Shift command queue + click markers | game | ⬜ |
| C7 | Attack, AttackMove: commands defined now, behaviour with combat ([04](04-combat.md)) | game | ⬜ |
| — | Patrol, rally points, double-click select-type, formations | game | ⬜ later |

Order of work: **C2 → C3 → C5**. C5 doesn't need C3, so the two can swap. C4 can come any time after C3.

---

## Implementation plan: C2 – Many units, teams, selection

**Goal:** a field of own and enemy units; select any of them with click, box and Shift; the
right click moves only the selected own units.

### Sim (`dotwars_sim`)
| Change | Detail |
|---|---|
| `Team(PlayerId)` component | Who owns the unit. Added to `checksum()`. |
| `Body { radius: Fixed }` component | Unit size. Used now for picking and drawing, later for separation (milestone 5). Added to `checksum()`. |
| `spawn_unit(pos, owner)` | Takes the owner; returns `UnitId`. |
| `UnitId → Entity` index | `BTreeMap<UnitId, Entity>` in `DotWars`, updated on spawn (and despawn, later). Replaces the O(n) scan when commands look units up. |
| `DotWars::demo_scenario(seed)` | Deterministic start: ~12 own units in a block on the left, ~8 enemy units on the right. Lives in the sim so tests and the client start from the same state. |

### Client (`dotwars_client`)
| File | Content |
|---|---|
| `view.rs` | `UnitView { id, team, pos: Vec2, radius: f32 }`, built each frame from the sim with **interpolated** positions. The one place that reads sim components for drawing and picking; everything below works on `&[UnitView]`. |
| `selection.rs` | `Selection(BTreeSet<UnitId>)` (sorted, so the Commands it produces are always identical). Pure functions, no macroquad: `pick(views, world_pos, tolerance) -> Option<UnitId>` and `box_select(views, rect, camera, local) -> Vec<UnitId>`. |
| `main.rs` | Calls the selection functions on `Gesture`s, prunes dead ids, draws team colors and selection circles. |

**Rules**
- **Click** picks the nearest unit whose `radius + tolerance` covers the cursor (tolerance ≈ 4 px ÷ zoom). Any team can be clicked, to inspect it, but only own units receive commands.
- **Box** selects **own units only**. An empty box result keeps the old selection unless Shift is held.
- **No Shift:** replace the selection. **Shift + click:** toggle that unit. **Shift + box:** add.
- **Click on empty ground** without Shift clears the selection.
- **Right click** sends `Move` with the own units in the selection. No own units → no command.
- Dead units are pruned from the selection every frame.

**Drawing:** own = blue, enemy = red; selected = a ring 3 px outside the body (green for own, yellow for enemy).

### Tests
- `selection.rs`: pick nearest of two overlapping units; pick misses outside radius + tolerance; box ignores enemies; Shift toggle/add; pruning.
- `dotwars_sim`: `demo_scenario` is deterministic (same checksum twice); `Team` and `Body` change the checksum.

### Done when
You can box-select part of the blue block, Shift-click to add one more, right-click, and only those units move. Clicking a red unit shows a yellow ring but right click does nothing.

---

## Implementation plan: C3 – Orders, Move/Stop/Hold, ownership

**Goal:** units carry a real order instead of a bare target; Stop and Hold work; the sim
refuses commands for units the player doesn't own.

### Sim (`dotwars_sim`)
**Components**
```rust
pub enum Order { Move(FixedVec2), Hold }

pub struct Orders {
    pub current: Option<Order>, // None = idle
    pub queue: Vec<Order>,      // filled by queue: true (C6 adds the Shift key for it)
}
```
- `MoveTarget` is removed; `Orders` is added at spawn (idle).
- **Stop** = clear `current` and `queue`. **Hold** = `current = Some(Hold)`, queue cleared. Until combat
  exists, Hold and Stop look the same. The difference comes in C7 (Hold never chases, Stop may auto-acquire).

**Commands**
```rust
pub enum Command {
    Move { units: Vec<UnitId>, target: FixedVec2, queue: bool },
    Stop { units: Vec<UnitId> },
    Hold { units: Vec<UnitId> },
}
```
`queue` is implemented in the sim now (it's a few lines): `false` replaces current and clears the queue,
`true` appends (or becomes current if idle). The client always sends `false` until C6.

**Applying a command (`apply`)** for each `UnitId`:
1. Look up the entity in the index; **missing (dead) → skip**.
2. `Team != issuing player` → **skip** (bug or cheat; with debug assertions on, log it).
3. Duplicates in `units` are ignored (dedupe via the sorted index walk).
4. Units are processed in `UnitId` order, so the result doesn't depend on the list order.

**Systems** (tick order): `store_prev_positions → apply commands → advance_orders → movement`.
- `movement` reads `Orders.current`; on `Move` it steps toward the target, and on arrival marks the order done.
- `advance_orders` moves finished orders out and takes the next from `queue` (or goes idle).

`checksum()` covers `Orders` (variant tag + target), in `UnitId` order.

### Client
- `controls.rs`: `Action::Stop` = **S**, `Action::Hold` = **H**. Both act on own selected units.
- Right click unchanged (sends `Move { queue: false }`).

### Tests (`dotwars_sim`, headless)
- **Ownership:** player 1 sends `Move` for player 0's unit → unit doesn't move, checksum equals a run without the command.
- **Stop** mid-move → unit stays where it is from the next tick on.
- **Queue:** `Move A` then `Move B, queue: true` → unit reaches A, then B.
- **Dead unit** in a command → no panic, other units still move.
- **Order independence:** `units: [3, 1, 2]` and `[1, 2, 3]` give the same checksum.
- Existing determinism test still passes.

### Done when
S stops a moving group instantly (after the input delay), H likewise, and all tests above pass.

---

## Implementation plan: C5 – Camera and control groups

**Goal:** move around a map larger than the window, zoom, and recall groups with number keys.

### Engine: `phalanx_input`
- `InputState::mouse_delta()`: mouse movement this frame (needed for middle-mouse drag). `begin_frame` stores the previous position.

### Engine: `phalanx_client::camera` (camera moves to its own module)
`Camera` stays (`offset`, `zoom`, `world_to_screen`, `screen_to_world`) and gains:
- `center_on(world_pos, screen_size)`
- `zoom_at(factor, screen_anchor)`: zooms so the world point under the cursor stays under the cursor.
- `clamp_to(bounds: Rect)`: the view can't leave the map.

`RtsCameraController` (settings + `update`), knowing nothing about game actions:
```rust
pub struct RtsCameraController {
    pub pan_speed: f32,     // screen px per second, same feel at any zoom
    pub edge_margin: f32,   // px from window edge that starts scrolling (0 = off)
    pub zoom_step: f32,     // factor per wheel notch, e.g. 1.15
    pub zoom_range: (f32, f32),
    pub bounds: Option<Rect>,
}
impl RtsCameraController {
    /// `pan_dir` comes from the game's key bindings (WASD/arrows), already normalized.
    pub fn update(&self, cam: &mut Camera, input: &InputState, pan_dir: [f32; 2],
                  screen_size: [f32; 2], dt: f32);
}
```
`update` handles: keyboard pan (`pan_dir`), edge scroll, middle-mouse drag (`mouse_delta`), wheel zoom toward the cursor, then clamps to `bounds`.

> **Known issue:** macroquad keeps reporting the last mouse position when the cursor leaves the window, so edge scroll keeps going. The setting `edge_margin = 0` turns it off; winit's `CursorLeft` event fixes it in R1.

### Game: `dotwars_client`
- `controls.rs`: `PanUp/Down/Left/Right` (**arrow keys only**: WASD would collide with S = Stop). The game turns held keys into `pan_dir` via `Bindings::is_held`.
- Map size: a `MAP_SIZE` constant for now (e.g. 2000 × 1500 world units) and a grid drawn every 100 units so movement is visible. The real map comes from [05](05-map.md).
- `groups.rs`: `ControlGroups { groups: [Vec<UnitId>; 10], last_tap: Option<(u8, f64)> }`:

| Keys | Action |
|---|---|
| Ctrl + N | assign selection to group N (replace) |
| Shift + N | add selection to group N |
| N | select group N |
| N twice within 0.3 s | select and center the camera on the group's centroid |

  Bindings for all ten digits are registered in a loop (`Key::digit()` exists for this). Dead units are pruned when a group is used. The time is passed in (`get_time()`), so the logic stays a pure, testable function.

### Tests
- `camera`: `zoom_at` keeps the anchor point fixed; `clamp_to` keeps the view inside bounds; world→screen→world round-trip; edge scroll moves only inside the margin.
- `phalanx_input`: `mouse_delta` is reset each frame.
- `groups.rs`: assign/select/add; pruning; double tap within and outside the time window.

### Done when
WASD, the window edges and the middle mouse move the camera over a grid larger than the window; the wheel
zooms toward the cursor; Ctrl+1, then 1, re-selects a group, and double-tapping 1 jumps to it.

---

## Status

**Implemented:** C0, C1, C2, C3, C5. (C4 formation movement, C6 Shift key for queued orders and click markers, C7 attack are next.)
- C0: `Command::Move`, straight-line movement, render interpolation, determinism test.
- C1: all input goes through `phalanx_input` (`backend::mq::poll` once per frame). `controls.rs` holds the game `Action` enum and default bindings.
- C2: `Team`, `Body`, `UnitId → Entity` index, `DotWars::demo_scenario` (12 own, 8 enemy units). Client: `view.rs` (interpolated `UnitView`), `selection.rs` (`pick`, `box_select`, `Selection`), team colors and selection rings.
- C3: `Orders { current, queue }` replaces `MoveTarget`. `Command::{Move { queue }, Stop, Hold}`. The sim ignores units the player doesn't own, dead units and duplicate ids; the result doesn't depend on list order. `queue: true` is already implemented in the sim (the client sends `false` until C6). `Orders` is part of `checksum()`.
- C5: `phalanx_input::mouse_delta`; `phalanx_client::camera` (`Camera::{center_on, zoom_at, clamp_to}`, `RtsCameraController`); client `groups.rs`, a 2000×1500 map with a grid.

**Differences from the plan**
- Pan keys are the arrow keys only (WASD collides with S = Stop). Rebind in `controls.rs`.
- `Selection::click` on empty ground with Shift does nothing (only plain click clears).
- Ctrl+N with an empty selection is ignored, so a stray press can't wipe a group.
- Right click does nothing when the selection has no own units.
- Dead units are pruned from the selection after every tick; groups prune when used.
- Tests: sim 10, client 16 (selection, groups, controls), `phalanx_client` camera 10, `phalanx_input` 12.

**Not verified by a human yet — check by hand (the "done when" lists above):**
1. C2: box-select part of the blue block, Shift-click one more, right-click: only those move. Click a red unit: yellow ring, right click does nothing.
2. C3: S stops a moving group, H holds it.
3. C5: arrows, window edges and middle mouse pan; wheel zooms toward the cursor; Ctrl+1 then 1 re-selects; double tap 1 jumps the camera.

**Next:** C4 (group movement keeping its shape), then C6.
