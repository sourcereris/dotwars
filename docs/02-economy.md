# 02 – Economy

Feature list: [GAME_DESIGN.md § Economy](../GAME_DESIGN.md#2-economy) · Depends on: [01 – Control](01-control.md) (C2 `Team`/`Body`, C3 `Orders`)

## Design

### The loop

```
        ┌──────────── Gather order ────────────┐
        ▼                                      │
  MovingToNode ──► Harvesting ──► Returning ──► Deposit ──► stockpile += cargo
        ▲          (N ticks)      (to nearest                 │
        │                          own drop-off)              ▼
        └──── node depleted? pick nearest other node ◄── Event::Deposited
```

Everything is in the sim, in ticks and integers. The client only draws it and turns a right click into a `Gather` command.

### Data (sim)

| What | Kind | Content |
|---|---|---|
| Resource node | entity | `ObjectId`, `Position`, `Body`, `ResourceNode { amount: u32, harvester: Option<ObjectId> }` |
| Drop-off | entity | `ObjectId`, `Position`, `Body`, `Team`, `DropOff` marker. For now a main building placed by the scenario; real buildings come in [03](03-production.md). |
| Worker | component | `Worker { carry: u32, capacity: u32, state: GatherState }` on a normal unit |
| Stockpile | **global** | `PlayerStock { minerals: u32 }` per player in a `BTreeMap<PlayerId, PlayerStock>` next to the `World` (not a component). |

```rust
enum GatherState {
    Idle,
    ToNode { node: ObjectId },
    Harvesting { node: ObjectId, ticks_left: u16 },
    Returning { node: ObjectId },   // remembers the node to go back to
}
```

> **📝 Decision – `UnitId` → `ObjectId`.** Resource nodes and buildings need stable ids too, and commands must be able to name them (`Gather { node }`). Rename `UnitId` to `ObjectId` once C2–C3 are merged: one id type for everything the sim tracks, one index (`BTreeMap<ObjectId, Entity>`).

### Numbers (placeholders, later in a `ron` file)
- Trip: **harvest 20 ticks (1 s)**, carry **5** minerals.
- Node: **1500** minerals. Start stock: **50**.
- All integers, no `Fixed` needed.

### Orders and commands
- New order: `Order::Gather(ObjectId)`. The gather state machine lives in the `Worker` component, not in the order, so the order stays small and comparable.
- New command: `Command::Gather { units: Vec<ObjectId>, node: ObjectId, queue: bool }`.
  Only units with `Worker` and owned by the player accept it. Other units in the list ignore it.
- `Move`/`Stop`/`Hold` on a worker cancels gathering and keeps the cargo. "Return cargo" is a later command.
- **Smart right click (client):** resource node + selection contains workers → `Gather` for the workers and `Move` to the node for the rest.

### Systems (tick order, inserted after `apply commands`)
> In the implementation plan these became one `gather` system + `depletion` (see *Changes to the design*). The logic below stays the same.

`gather_targeting → movement → harvest → deposit → depletion`
1. **gather_targeting:** for workers in `ToNode`/`Returning`, set the movement target (node or nearest own drop-off).
2. **movement:** unchanged; just moves toward the target.
3. **harvest:** a worker that reached its node claims the slot (`harvester`). If it's taken, see crowding. Counts `ticks_left` down, then moves `min(capacity, amount)` into `carry` and switches to `Returning`.
4. **deposit:** a worker in `Returning` that reached a drop-off adds `carry` to its player's stock, emits `Event::Deposited { player, amount }`, then goes back `ToNode`.
5. **depletion:** a node with `amount == 0` is despawned and emits `Event::NodeDepleted { node }`. Its workers switch to the nearest other node within a search radius, or go `Idle`.

### "Nearest" must be deterministic
"Nearest node" and "nearest drop-off" use `dist_sq` (`i64`), with ties broken by `ObjectId`. A plain `min_by_key` over a hecs query is **not** enough, because query order isn't stable. Combat needs exactly the same thing (nearest enemy), which makes it the second use, so `nearest_by(candidates, pos, key)` moves to `phalanx_sim` when combat starts.

### Crowding (MVP rule)
- **One harvester per node** at a time (`harvester` slot).
- A worker that arrives at an occupied node waits up to **10 ticks**, then retargets the nearest *free* node within the search radius. If none is free, it keeps waiting.
- Claims are resolved in `ObjectId` order within a tick, so two workers arriving on the same tick always resolve the same way.
- No pathing between nodes yet, so workers can still overlap each other while walking. That's fixed by separation (milestone 5).

### Events → client
| Event | Client reaction |
|---|---|
| `Deposited { player, amount }` | "+5" floating text at the drop-off (own player only), counter update |
| `NodeDepleted { node }` | small effect, later a sound |

### Client
- Draw nodes as circles whose size shrinks with `amount` (gold color). Draw a drop-off as a big square in the team color.
- A worker carrying cargo gets a small dot on top.
- Top bar: `Minerals: N` for the local player (read from the sim stockpile).

### Checksum
Add the stockpile (in `PlayerId` order), `Worker` (carry + state tag + ids + ticks) and `ResourceNode` (amount + harvester).

### Testing (headless)
- **One trip:** a worker next to a node and a drop-off gets exactly +5 after the expected number of ticks.
- **Depletion:** a node with 7 minerals → trips of 5 and 2 → node despawned, `NodeDepleted` emitted, worker retargets or goes idle.
- **Contention:** two workers reach one node on the same tick → the lower `ObjectId` harvests; the other retargets after 10 ticks. Same result every run.
- **Ownership:** a player can't order the enemy's workers to gather (reuses the C3 check).
- **Stop keeps cargo:** stop a returning worker, then `Gather` again → it returns the cargo first.
- **Checksum** changes when the stock changes; the determinism test with 10 workers × 2 nodes passes.

### Engine or game?
Almost everything here is **game** (`dotwars_sim`). Two candidates for Phalanx, both per the rule of two:
- `nearest_by` (deterministic nearest with an id tie-break): moves to the engine when combat needs it.
- A tick-based `Cooldown`/`Timer` helper: harvest uses it now, and attack cooldowns will be the second use.

## Steps

| Step | What | Needs | Status |
|---|---|---|---|
| E0 | Rename `UnitId` → `ObjectId` (one index for units, buildings, resources) | C2–C3 ✅ | ✅ |
| E1 | Resource nodes + drop-off placed by `demo_scenario`; `PlayerStock`; drawn in the client; top-bar counter | E0 | ✅ |
| E2 | `Worker` component, `Order::Gather`, `Command::Gather`; one worker, one node: full trip and deposit | E1, C3 | ✅ |
| E3 | Depletion, `NodeDepleted`, retarget to the nearest node (deterministic `nearest_by`) | E2 | ✅ |
| E4 | Crowding: one harvester per node, wait, then retarget | E3 | ✅ |
| E5 | Smart right click on a node; `Deposited` → floating "+5"; carry dot | E2 | ✅ |
| — | Return-cargo command, supply/population cap, second resource, expansions | [03](03-production.md) | ⬜ later |

**Done when (E1–E5):** select 5 workers, right-click a mineral node. They spread across nearby nodes, walk back and forth, and the counter goes up by 5 per trip. Nodes shrink and disappear, and the workers move on by themselves.

---

## Implementation plan

Starting point (after C2, C3, C5): `UnitId` + `index: BTreeMap<UnitId, Entity>`, `Orders { current, queue }`,
`Order::{Move, Hold}`, the systems `store_prev_positions → apply → advance_orders → movement`, `Event` is empty,
and the client throws events away (`events.clear()`).

### Changes to the design (found while planning)
- **One `gather` system instead of five.** Claiming a node touches *another* entity (the node's `harvester` slot), so
  workers must be processed in `ObjectId` order, not in query order. One system that walks workers sorted by id and
  steps each one's state machine is simpler than five systems that each have to sort. Order:
  `store_prev_positions → apply → advance_orders → movement → gather → depletion`.
- **The gather state follows the order, not the command.** `gather` itself checks for an `Order::Gather` current
  order with an `Idle` state (start), and for a non-Gather order with a busy state (cancel). So queued `Gather`
  orders and `Move/Stop/Hold` over a gather need no special code in `apply`.
- **`nearest` needs no sorted input.** `min_by_key(|c| (dist_sq, id))` gives the same answer for any input order,
  because the key is unique. A plain hecs query is enough as long as the tie-break id is in the key.
- **Events carry positions.** `NodeDepleted` is emitted *after* the node is gone, so the client gets `pos` in the
  event. `Deposited` names the drop-off so the "+5" knows where to appear.
- **Stale claims are free.** A `harvester` slot counts as taken only if that worker still exists and is
  `Harvesting` that node. Dead or reassigned workers never need cleanup code.

### E0 – `UnitId` → `ObjectId`
| Where | Change |
|---|---|
| `components.rs` | `pub struct ObjectId(pub u32)` with the same derives. No alias left behind. |
| `lib.rs` | `index: BTreeMap<ObjectId, Entity>`; `entity`, `despawn_unit` → `despawn`. `sorted_units()` keeps its name but filters on `Orders`, so nodes and buildings don't show up as units; add `sorted_objects()` for everything. |
| client | `view.rs`, `selection.rs`, `groups.rs`, `main.rs`: rename only. `build_views` still requires `Orders` (implied by `sorted_units`). |

Pure rename and filter. All existing tests pass unchanged, except for the renamed type.

### E1 – Nodes, drop-off, stockpile
**Sim**
- Components: `ResourceNode { amount: u32, harvester: Option<ObjectId> }`, `DropOff` (marker).
- `DotWars` gets `stock: BTreeMap<PlayerId, PlayerStock>`, `pub fn stock(&self, p) -> u32` (0 for unknown players).
- Spawners (each calls the same internal `next_object(components)` that takes the id and indexes it):
  - `spawn_node(pos, amount) -> ObjectId`: `Position`, `Body { radius: 12 }`, `ResourceNode`. No `Team`, no `Orders`.
  - `spawn_dropoff(pos, owner) -> ObjectId`: `Position`, `Body { radius: 24 }`, `Team`, `DropOff`.
- Constants in one block in `economy.rs` (new module): `NODE_AMOUNT = 1500`, `START_STOCK = 50`,
  `HARVEST_TICKS = 20`, `CARRY = 5`, `WAIT_TICKS = 10`, `SEARCH_RADIUS = 400`.
- `demo_scenario`: for each player, a drop-off, a field of 6 nodes in an arc 150–200 units away, and the
  start stock. P0's base sits at (250, 500); P1 mirrors it at (1750, 500). Existing unit blocks stay.
- Checksum: stock in `PlayerId` order. Objects are walked through `sorted_objects()`, with `ResourceNode`
  (amount, harvester or `u32::MAX`) and a `DropOff` tag hashed next to the existing per-unit fields.

**Client**
- `view.rs`: `NodeView { id, pos, radius, amount }`, `DropOffView { id, team, pos, radius }`.
  They don't move, so no interpolation.
- Draw nodes as gold circles with radius `12 · sqrt(amount / 1500)`, at least 4. Draw drop-offs as squares in the team color,
  under the units.
- Top bar: `Minerals: N` for `LOCAL_PLAYER` from `driver.sim.stock(..)`.

**Tests:** the scenario is deterministic and has 2 drop-offs, 12 nodes and stock 50/50. Changing a node amount changes
the checksum.

### E2 – Workers and one full trip
**Sim**
- `Worker { carry: u32, state: GatherState }` (capacity is the `CARRY` constant for now).
  ```rust
  enum GatherState {
      Idle,
      ToNode { node: ObjectId, waited: u16 },
      Harvesting { node: ObjectId, ticks_left: u16 },
      Returning { node: ObjectId },
  }
  ```
- `spawn_worker(pos, owner)` = `spawn_unit` + `Worker::default()` + `Body { radius: 6 }`. `demo_scenario`
  adds 5 workers next to each drop-off.
- `Order::Gather(ObjectId)` and `Command::Gather { units, node, queue }`. `apply` reuses `owned_targets`, then
  keeps only entities that have `Worker`. A node id that doesn't exist or isn't a `ResourceNode` → the command is ignored.
- Refactor `movement`: pull out `fn step_toward(pos: &mut FixedVec2, target: FixedVec2, stop_at: Fixed) -> bool`
  (true = arrived, i.e. `dist_sq <= stop_at²`). `Order::Move` uses `stop_at = SPEED` as today.
- `systems::gather(world, index, stock, events)`, walking workers in `ObjectId` order:
  1. **Sync with the order.** Current order is `Gather(n)` and the state is `Idle` → `Returning { n }` if `carry > 0`, else `ToNode { n, 0 }`.
     Current order is not `Gather` and the state is not `Idle` → `Idle` (cargo kept, claim simply goes stale).
  2. **ToNode:** if the node is gone → retarget (E3; in E2, go `Idle` and clear the order). Otherwise
     `step_toward(node pos, r_node + r_worker)`. On arrival: claim it if the slot is free (or stale) → `Harvesting { n, HARVEST_TICKS }`.
     If taken → E4 (in E2, wait).
  3. **Harvesting:** `ticks_left -= 1`; at 0, `take = min(CARRY, amount)`, `amount -= take`, `carry += take`,
     release the slot → `Returning { n }`.
  4. **Returning:** `nearest` own `DropOff`. If there is none, stand still. Otherwise `step_toward(r_drop + r_worker)`. On arrival:
     `stock[team] += carry`, emit `Deposited { player, dropoff, amount }`, `carry = 0` → `ToNode { n, 0 }`.
- `fn nearest(from, candidates: impl Iterator<Item = (ObjectId, FixedVec2)>) -> Option<ObjectId>` by key `(dist_sq, id)`.
- `Event::{Deposited { player, dropoff: ObjectId, amount: u32 }, NodeDepleted { node: ObjectId, pos: FixedVec2 }}`.
- Checksum: `Worker` (carry, state tag, ids, counters) and the `Gather` order tag.

**Tests**
- One trip: worker, node and drop-off on a line. Stock is 50 up to tick T−1 and 55 at tick T, where the test derives T
  from the input delay + walk ticks + `HARVEST_TICKS` + walk back (written out in a comment).
- Two trips → 60, and the node has 1490.
- `Gather` for an enemy worker, a non-worker unit, or a non-node target changes nothing (checksums match a clean run).
- Stop during `Returning`, then `Gather` again → it goes to the drop-off first (cargo kept).
- `Move` during `Harvesting` frees the slot: a second worker can claim it.
- Determinism: 10 workers × 2 nodes, 600 ticks, same checksums twice.

### E3 – Depletion and retarget
- `systems::depletion(world, index, events)`, after `gather`: nodes with `amount == 0`, sorted by id → emit
  `NodeDepleted { node, pos }`, then despawn and remove from the index.
- `gather` step 2, node gone → `nearest` node with `amount > 0` within `SEARCH_RADIUS` of the worker. Found → `ToNode { new, 0 }`
  and update the order to `Gather(new)`. Not found → `Idle`, `orders.current = None`. A worker in `Returning` whose node is gone still
  deposits first, then retargets.
- **Tests:** a node with 7 minerals → trips of 5 and 2 → `NodeDepleted` once, the node is gone from the index, and the worker
  moves to the second node. With no second node in range, the worker is idle and has no order.

### E4 – Crowding
- `gather` step 2, slot taken → `waited += 1`. At `WAIT_TICKS`, `nearest` node in range whose slot is free (or stale) and
  isn't the current one. Found → `ToNode { new, 0 }` + order update. None → `waited = 0` and keep waiting.
- Same-tick claims are resolved by the `ObjectId` walk order: the lower id claims first.
- **Tests:** two workers arrive on the same tick → the lower id harvests and the other moves on after 10 ticks; same result
  over 5 runs. 6 workers on 3 nodes all end up harvesting with no deadlock (each node is busy at some tick).

### E5 – Client feedback
- **Smart right click:** pick a `NodeView` under the cursor (same `pick` rule: nearest, then the lower id).
  If the selection holds own workers, workers get `Gather`, everyone else gets `Move` to the node position. No node
  under the cursor → `Move` as before.
  `UnitView` gets `worker: bool, carrying: bool`.
- **Events:** the client keeps `events` per frame instead of `clear()` and hands them to `effects.rs` (new). Effects
  are client-only and in real time (f32 OK):
  - `Deposited` for `LOCAL_PLAYER` → floating "+5" over the drop-off, rising and fading over 1 s.
  - `NodeDepleted` → a ring that grows and fades at `pos` over 0.5 s.
- A carrying worker gets a small gold dot. The debug overlay gets `Minerals`, number of nodes, and the gather state of the
  first selected worker.
- **Tests:** a smart click on a node with a mixed selection splits into `Gather` + `Move` (pure function
  `right_click_commands(views, nodes, selection, world_pos) -> Vec<Command>` in a new `orders.rs`).
  The effect lifetime ends and the effect is removed.

### Done when (E1–E5)
See **Done when** under Steps, checked by hand in the game. All sim tests and clippy pass.

## Status

**Implemented:** E0–E5. Not verified by a human yet (see the checklist below).
- E0: `UnitId` → `ObjectId` everywhere; `sorted_objects()` (all) vs `sorted_units()` (only objects with `Orders`).
- E1: `ResourceNode`, `DropOff`, per-player `stocks` (`stock()`, `set_stock()`), `spawn_node/dropoff/worker`. `demo_scenario` adds a base for each player at (250, 500) and (1750, 500): drop-off, 6 nodes, 5 workers, stock 50. Constants live in `dotwars_sim::economy`. Client: `NodeView`/`DropOffView`, gold nodes that shrink, team-colored drop-off squares, `Minerals: N` top bar.
- E2: `Worker`, `GatherState`, `Order::Gather`, `Command::Gather` (ignored for non-workers, enemy units and non-nodes). One `gather` system walks workers in `ObjectId` order. Checksum covers workers, nodes, drop-offs and stock.
- E3: `depletion` removes empty nodes with `Event::NodeDepleted { node, pos }`; workers retarget to the nearest node within 400 units, or go idle and drop the order.
- E4: one harvester per node; a worker that waits 10 ticks at an occupied node moves to the nearest free one. Claims by workers that left count as free.
- E5: smart right click (`orders.rs`: workers gather, the rest walk to the node), `effects.rs` ("+5" text, depletion ring), gold dot on carrying workers, debug line with minerals, node count and the first selected worker's state.

**Differences from the plan**
- `Worker` has no `capacity` field; `CARRY` is a constant until unit data files exist.
- `step_toward` is separate from `movement` (which stays exactly as it was, so Move behaves the same and the old tests are untouched).
- The node search is limited to 400 units; the drop-off search has no limit.
- Tests: sim 23 (12 new in `economy.rs`), client 23 (`orders`, `effects`).

**Check by hand:**
1. Box-select the 5 blue workers near the blue square, right-click a gold node: they walk, mine (1 s), return, and `Minerals` goes up by 5 per trip with a floating "+5".
2. Workers carrying minerals show a gold dot; a node shrinks as it empties.
3. Send all 5 to one node: one mines, the others wait ~0.5 s and spread to other nodes.
4. Select workers plus a soldier and right-click a node: workers mine, the soldier walks there.
5. S while a worker walks back: it stops with its cargo; right-click the node again and it delivers first.
6. Debug overlay (F1) shows the first selected worker's state.

**Next:** [03 – Production](03-production.md) design (buildings, construction, queue), then supply.
