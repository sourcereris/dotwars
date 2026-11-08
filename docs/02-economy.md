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
| E0 | Rename `UnitId` → `ObjectId` (one index for units, buildings, resources) | C2–C3 merged | ⬜ |
| E1 | Resource nodes + drop-off placed by `demo_scenario`; `PlayerStock`; drawn in the client; top-bar counter | E0 | ⬜ |
| E2 | `Worker` component, `Order::Gather`, `Command::Gather`; one worker, one node: full trip and deposit | E1, C3 | ⬜ |
| E3 | Depletion, `NodeDepleted`, retarget to the nearest node (deterministic `nearest_by`) | E2 | ⬜ |
| E4 | Crowding: one harvester per node, wait, then retarget | E3 | ⬜ |
| E5 | Smart right click on a node; `Deposited` → floating "+5"; carry dot | E2 | ⬜ |
| — | Return-cargo command, supply/population cap, second resource, expansions | [03](03-production.md) | ⬜ later |

**Done when (E1–E5):** select 5 workers, right-click a mineral node. They spread across nearby nodes, walk back and forth, and the counter goes up by 5 per trip. Nodes shrink and disappear, and the workers move on by themselves.

## Status

**Implemented:** nothing yet. The design is written; implementation starts after C2–C3 are merged.
