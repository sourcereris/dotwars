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
with a list of `UnitId`s.

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
    Move       { units: Vec<UnitId>, target: FixedVec2, queue: bool },
    AttackMove { units: Vec<UnitId>, target: FixedVec2, queue: bool },
    Attack     { units: Vec<UnitId>, target: UnitId,    queue: bool },
    Stop       { units: Vec<UnitId> },
    Hold       { units: Vec<UnitId> },
}
```
- **Ownership check:** the sim ignores units that don't belong to the issuing player (guards against bugs and cheats). Needs a `Team` component.
- Units that died between issue and execution are skipped.

### Orders (sim)
`Orders` component = current order + queue (`Vec<Order>`), replacing today's `MoveTarget`.
- `queue: false` clears the queue and replaces the current order; `queue: true` appends.
- Systems read only the current order; when it completes, the next one starts.
- `Order::{Move(pos), AttackMove(pos), Attack(UnitId), Hold}`. No order = idle.

### Group movement (sim, deterministic)
20 units sent to one point would fight over one pixel. MVP: keep the group's shape.
Each unit targets `target + (own_pos − group_center)`, with the offset clamped to a maximum radius.
Computed in the sim from the command, so commands stay small. Proper formations: later.

### Control groups (client)
`[Vec<UnitId>; 10]`. Ctrl+N assigns, N selects, double-tap N centers the camera. Dead units pruned.

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
| C2 | Many units, `Team`, click/box/Shift selection, selection circles | game | ⬜ |
| C3 | `Orders` replaces `MoveTarget`; Move/Stop/Hold; ownership check | game sim | ⬜ |
| C4 | Group movement keeping formation shape | game sim | ⬜ |
| C5 | Camera pan/zoom + control groups | engine + game | ⬜ |
| C6 | Shift command queue + click markers | game | ⬜ |
| C7 | Attack, AttackMove: commands defined now, behaviour with combat ([04](04-combat.md)) | game | ⬜ |
| — | Patrol, rally points, double-click select-type, formations | game | ⬜ later |

## Status

**Implemented:** C0, C1.
- C0: `Command::Move` with a `MoveTarget` component, straight-line movement, render interpolation, determinism test.
- C1: all input goes through `phalanx_input` (`backend::mq::poll` once per frame). Right click moves on *press*. Left drag shows a box, left click/box is recorded in the debug overlay. `controls.rs` holds the game `Action` enum and default bindings (F1 = toggle debug overlay).
interpolation, determinism test. Input read directly from macroquad in `dotwars_client`.

**Next:** C2 (many units, `Team`, selection).
