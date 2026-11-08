# 03 – Production

Feature list: [GAME_DESIGN.md § Production](../GAME_DESIGN.md#3-production) · Depends on: [02 – Economy](02-economy.md)
(E1 `PlayerStock`, E2 `Worker`) · Feeds: [04 – Combat](04-combat.md) (what to train), [05 – Map](05-map.md) (the grid)

## Design

### The loop

```
 worker selected ──► build key ──► ghost on grid (client) ──► click ──► Command::Build
                                                                          │ (3 ticks later)
                                                                          ▼
                          worker walks to the site ──► arrives: valid + affordable? ──no──► BuildFailed
                                                                          │ yes
                                                                          ▼
                     cost paid, cells taken, building spawned (progress 0) ──► worker constructs N ticks
                                                                          │
                                                                          ▼
                    BuildingFinished ──► building select ──► train key ──► Command::Train ──► queue
                                                                          │
                                                       front item done ──► unit spawns ──► walks to rally point
```

Everything that decides anything is in the sim. The client draws the ghost and tells the player in advance
whether placement will work, using the **same pure function** the sim uses.

### Engine: grid (`phalanx_sim::grid`)
The first engine-level map structure. Placement uses it now, and pathfinding (milestone 6) and fog of war (05)
will use it later, so it lives in Phalanx from the start (three known uses, not a guess).

```rust
pub struct Cell { pub x: u16, pub y: u16 }
pub struct Grid<T> { width: u16, height: u16, cell_size: Fixed, cells: Vec<T> }   // row-major

impl<T: Copy> Grid<T> {
    pub fn new(width: u16, height: u16, cell_size: Fixed, fill: T) -> Self;
    pub fn get(&self, c: Cell) -> Option<T>;            // None = out of bounds
    pub fn set(&mut self, c: Cell, v: T);
    pub fn cell_at(&self, p: FixedVec2) -> Option<Cell>;
    pub fn cell_origin(&self, c: Cell) -> FixedVec2;    // top-left corner in world units
    pub fn rect(&self, c: Cell, w: u8, h: u8) -> Option<impl Iterator<Item = Cell>>;  // None if it leaves the map
}
```
- Generic over `T`, so the engine never learns what an `ObjectId` is. Dot Wars uses `Grid<Option<ObjectId>>`.
- Fixed-point and integer only, `Vec` storage, no hashing, so it's deterministic by construction.
- Walls and cliffs (05) will become a second grid or a flag in `T`. Not now.

### Map size
Cell = **32** world units. The map grows from 2000×1500 to **2048×1536** = **64×48 cells**, so the grid covers it
exactly. The camera clamp (C5) follows.

### Data (sim, game)
| What | Kind | Content |
|---|---|---|
| Building | entity | `ObjectId`, `Position` (footprint center), `Body` (half the footprint's short side), `Team`, `Building { kind, cell }` |
| Under construction | component | `Construction { done: u16, total: u16 }`, removed when finished |
| Producer | component | `Producer { queue: Vec<UnitKind> (max 5), progress: u16 }`, `Rally(FixedVec2)`; only on finished buildings that train |
| Drop-off | component | `DropOff` from [02](02-economy.md), now added to a finished **Base** |
| Occupancy | **global** | `grid: Grid<Option<ObjectId>>` next to `World`, written on placement, cleared on despawn |
| Unit kind | component | `UnitKind::{Worker, Soldier}` on every unit, so supply and stats can be looked up |

Stats are const tables in `data.rs` (later `ron`, see *Later*):

```rust
pub struct BuildingDef { cost: u32, build_ticks: u16, size: (u8, u8), supply: u16, trains: &'static [UnitKind], drop_off: bool }
pub struct UnitDef     { cost: u32, train_ticks: u16, supply: u16, radius: Fixed }
pub fn building(k: BuildingKind) -> &'static BuildingDef;
pub fn unit(k: UnitKind) -> &'static UnitDef;
```

### Numbers (placeholders)
| Kind | Cost | Time | Size | Supply | Trains |
|---|---|---|---|---|---|
| Base | 400 | 1200 ticks (60 s) | 3×3 | +10 | Worker |
| Barracks | 150 | 600 ticks (30 s) | 3×3 | – | Soldier |
| Supply depot | 100 | 400 ticks (20 s) | 2×2 | +8 | – |
| Worker | 50 | 240 ticks (12 s) | – | 1 | – |
| Soldier | 75 | 360 ticks (18 s) | – | 2 | – |

Supply cap is at most **100**. Soldier stats (HP, damage) come in [04](04-combat.md); here it is only a cost and a body.

### Placement rules
`can_place(sim, player, kind, cell) -> Result<(), BuildError>` is one **pure** function in `dotwars_sim`.
It checks:
1. The footprint is inside the grid (`OutOfBounds`).
2. Every footprint cell is `None` (`Blocked`).
3. The stock is at least `cost` (`NotEnoughMinerals`).

**Units on the footprint don't block** (MVP). Units walking past would make placement fail at random. Separation
(milestone 5) will push them out of the building later. Resource nodes are written into the grid as well (1×1), so
no one builds on minerals.

The client calls `can_place` on the read-only sim for the green or red ghost. **The sim calls it again on arrival.**
The client's answer is only a preview: between the click and the arrival, the money or the cells may have
changed.

### Commands and orders
| Command | Who | Effect |
|---|---|---|
| `Build { worker, kind, cell, queue }` | one own worker | `Order::Build { kind, cell }`: walk to the site |
| `Construct { units, building, queue }` | own workers | `Order::Construct(id)`: resume an unfinished own building |
| `Train { building, kind }` | own finished producer | push to the queue (see *Production*) |
| `CancelTrain { building, slot }` | own producer | remove the item, full refund |
| `SetRally { buildings, target }` | own producers | `Rally(target)` |

- **`Order::Build`** walks until the worker touches the footprint's edge. Then the sim calls `can_place`:
  - **ok:** pay, write the cells, spawn the building with `Construction { done: 0 }`, and switch the order to `Construct(id)`.
  - **error:** emit `BuildFailed { player, reason }`; the worker goes idle.
- **`Order::Construct(id)`:** while touching the building, `done += 1` every tick. At `total`, remove `Construction`, add
  `Producer`/`DropOff` per the def, emit `BuildingFinished { id }`, and the worker goes idle. **One builder per building** in
  MVP (the first in `ObjectId` order); the others wait.
- Moving the builder away **pauses** construction. The building stays half-built until a worker resumes it
  (smart right click on an own unfinished building → `Construct`).
- A building can only be named by its own player; enemy ids or unfinished producers are ignored (C3 rule).

### Production
- **Cost is paid at `Train`** (enqueue), and `CancelTrain` refunds it fully. Nothing gets lost and nothing waits half-paid.
- **Supply is checked at `Train`**: `used + queued + unit.supply <= cap`, otherwise `TrainFailed { SupplyBlocked }`.
  `used` counts alive units and **queued items everywhere**, so two buildings can't both take the last supply. It is
  computed from the world each time it's needed; it's a sum, so order doesn't matter.
- Queue full (5) → `TrainFailed { QueueFull }`. Not enough minerals → `TrainFailed { NotEnoughMinerals }`.
- Each tick, the front item gets `progress += 1`. At `train_ticks`, the unit spawns on the building's edge facing the rally
  point (`pos + dir_to_rally · (body + unit radius)`, or straight down if the rally point is on the building). It gets
  `Order::Move(rally)`, and `UnitTrained { id, building }` is emitted.
- No rally set → the unit stays where it spawned.

### Systems (tick order)
`store_prev → apply → advance_orders → movement → gather → depletion → build → construct → produce`
- **build:** workers with `Order::Build` in `ObjectId` order (two workers can try to take the same cells in one tick:
  the lower id wins, the other gets `Blocked`).
- **construct:** buildings with `Construction` in `ObjectId` order; the first touching builder adds progress.
- **produce:** producers in `ObjectId` order, since spawning takes new ids and the id order must be fixed.

### Events → client
| Event | Client reaction |
|---|---|
| `BuildFailed { player, reason }` | red text in the middle of the screen ("Not enough minerals", "Can't build there") for the local player |
| `TrainFailed { player, reason }` | same, plus "Supply blocked" |
| `BuildingFinished { id }` | small flash on the building |
| `UnitTrained { id, building }` | none in MVP (sound later) |

### Client
- **Build mode** (client-only state): with own workers selected, `B` → Barracks, `V` → Supply depot, `N` → Base.
  - The ghost footprint follows the cursor, snapped to the grid, green or red from `can_place`.
  - Left click sends `Build` for the **nearest selected worker** to the site (nearest by distance, then the lower id).
  - Shift + left click places and keeps build mode on; right click or `Esc` cancels.
  - Keys move to the command card in [06](06-interface.md); rebinding is in `controls.rs`.
- **Building selected:** `Q` trains the first `trains` entry (Worker or Soldier). Right click sets the rally point; it is drawn
  as a line and a flag while the building is selected.
- **Drawing:** buildings are team-colored squares the size of the footprint, darker with a progress bar while under
  construction. Queue: "Soldier 3/5, 40%" text above the building while selected (icons in 06).
- **Top bar:** `Minerals: N   Supply: used/cap`.
- **Picking:** clicking a building selects it alone. Box select only takes units (it doesn't mix units and buildings).

### Checksum
Hash `Building` (kind, cell), `Construction`, `Producer` (queue + progress), `Rally`, `UnitKind`, and the whole grid
(64×48 values; cheap).

### Testing (headless)
- **Grid (engine):** bounds, `cell_at`/`cell_origin` round trip, a rect at the map edge returns `None`.
- **Placement:** on a free cell → ok, the cells are written and the stock goes down. On a node, on another building, or out of the map →
  `BuildFailed` with the right reason, and nothing is paid.
- **Race:** two workers build on the same cells in the same tick → the lower id wins, the other fails `Blocked`. Same every run.
- **Money changes in flight:** enough money at the click, spent on training before the worker arrives → `NotEnoughMinerals`.
- **Construction:** exactly `total` ticks of a builder touching the building → `BuildingFinished`; moving the builder away pauses it;
  `Construct` from another worker resumes from the same `done`.
- **Production:** train → pay → the unit spawns after `train_ticks` and walks to the rally point. Cancel → full refund.
  A queue of 5, then a 6th → `QueueFull`.
- **Supply:** the cap is reached with units + queued items → `SupplyBlocked`; a finished depot raises the cap.
- **Ownership:** an enemy can't train in, rally, or construct my buildings.
- **Determinism:** a scripted build order (depot, barracks, 5 soldiers) runs twice with equal checksums.
- **Grid consistency (debug test):** after the scripted build order, a grid rebuilt from the buildings and nodes equals `sim.grid`.

### Engine or game?
| Part | Where | Why |
|---|---|---|
| `Grid<T>`, `Cell` | **engine** (`phalanx_sim::grid`) | placement, pathing, fog: three uses |
| Build rules, `can_place`, defs, supply | game | Dot Wars rules |
| Ghost snapping math (world ↔ cell for the client) | engine, on `Grid` | `cell_at` + `cell_origin` are already there |
| Data loading from `ron` | engine later | any game needs it; not until defs actually move out of code |
| Production queue | game for now | candidate for the engine if a second game wants the same shape (rule of two) |

## Steps

| Step | What | Needs | Status |
|---|---|---|---|
| P0 | **Engine:** `phalanx_sim::grid` (`Grid<T>`, `Cell`) + tests | — | ✅ |
| P1 | `UnitKind`, `BuildingKind`, `data.rs`. `Building` entity; the scenario's drop-off becomes a finished **Base**; `sim.grid` with bases and nodes; map 2048×1536; buildings drawn | P0, E1 | ⬜ |
| P2 | `Command::Build`/`Construct`, `Order::Build`/`Construct`, `can_place`, pay on arrival, `Construction`, `BuildFailed`, `BuildingFinished` | P1, E2 | ⬜ |
| P3 | Client build mode: keys, ghost (green/red), place, Shift keeps mode, failure text, construction bar | P2 | ⬜ |
| P4 | Supply: cap from finished buildings, used + queued, top bar | P1 | ⬜ |
| P5 | `Producer`, `Train`/`CancelTrain`/`SetRally`, spawn at the edge facing the rally point, `UnitTrained`, `TrainFailed` | P2, P4 | ⬜ |
| P6 | Client: select a building, `Q` to train, right click rally, queue text, smart right click on an unfinished building → `Construct` | P5, E5 | ⬜ |
| — | Tech requirements (Barracks needs a depot), upgrades, defs in `ron`, cancel construction (75 % refund), several builders, buildings destroyed (with [04](04-combat.md)) | P6 | ⬜ later |

**Done when (P0–P6):** with 50 start minerals and 5 workers gathering, build a supply depot and a barracks, train 5
soldiers with a rally point, and watch them walk to it. Trying to build on minerals or with too little money shows the
reason. The supply counter blocks training at the cap.

## Status

**Implemented:** P0. **Next:** P1 (waits for E1).
- P0: `phalanx_sim::grid` — `Cell`, `Grid<T>` with `get`/`set` (bounds-checked), `cell_at` (floor on raw fixed-point,
  negatives → `None`), `cell_origin`, `rect_center` (footprint center), `rect` (row-order cells, `None` if it leaves the grid
  or is empty, overflow-safe), `rect_all(pred)`, `fill_rect` (all or nothing), `as_slice` (checksum). 7 tests.
- Difference from the design: `set` returns `bool` instead of silently ignoring out-of-bounds; `rect_center`, `rect_all`,
  `fill_rect` added because `can_place` and placement need exactly those.
The implementation plan (exact files and signatures per step) gets written once E1–E2 have landed, so it matches the
real economy code.
