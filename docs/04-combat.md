# 04 – Combat

Feature list: [GAME_DESIGN.md § Combat](../GAME_DESIGN.md#4-combat) · Depends on: [01 – Control](01-control.md)
(C7 `Attack`/`AttackMove`), [03 – Production](03-production.md) (P1 `UnitKind` + `data.rs`; buildings) ·
Feeds: [07 – AI](07-ai.md), [08 – Match flow](08-match-flow.md) (win/lose)

## Design

### The loop

```
 acquire target ──► in range? ──no──► chase (or not: Hold, leash) ──┐
       ▲                │ yes                                       │
       │                ▼                                           │
       │        cooldown ready? ──yes──► hit (damage queued) ───────┤
       │                                                            ▼
       └──── target dead / out of leash ◄──── death pass: hp 0 → Died, despawn, cleanup
```

Everything is integer and in ticks: hp, damage, cooldown, and range (`Fixed`). Hits are **instant** in the MVP;
the client draws a tracer, so ranged attacks still read as shots. Projectiles come later.

### Fairness: two-phase combat
If attackers deal damage one by one in `ObjectId` order, the lower id always shoots first. In an even 1v1 it then
kills before the other can answer, and the result depends on spawn order, not on the fight.

So the `combat` system runs in two passes over a **snapshot of the start of the tick**:
1. **Decide:** every attacker in `ObjectId` order picks or validates a target, decides whether to move, and decides
   whether to hit. It reads only start-of-tick positions and hp, and writes nothing to the world.
2. **Apply:** all moves, then all damage (`hp = hp.saturating_sub(dmg)`).

Then the `death` system removes everything at 0 hp. Two equal units that hit each other on the same tick
**both** die. Order no longer matters, and the test proves it by swapping spawn order.

### Data (sim)
| What | Kind | Content |
|---|---|---|
| Health | component | `Health { hp: u32, max: u32 }`, on every unit and building |
| Combat state | component | `Combat { cooldown: Cooldown, target: Option<ObjectId>, anchor: Option<FixedVec2>, retarget_in: u8 }`, on units that can attack |
| Last attacker | component | `LastHit { by: ObjectId, tick: u32 }`, set in phase 2, read by targeting ("attacker of me") |
| Stats | `data.rs` | `UnitDef` from P1 grows: `hp, damage, range, cooldown, speed, acquire, tag, bonus: Option<(Tag, u32)>` |

Stats are looked up via `UnitKind` each time, not copied into components. Upgrades ([03](03-production.md) *later*)
will turn them into components when they really have to change per unit.

`speed` replaces the global `SPEED` constant in `movement`/`step_toward`.

### Units and counters (MVP)
Each attack type has a **bonus against one tag**. That alone is enough for a counter triangle; full armor values come later.

| Unit | Tag | HP | Damage | Bonus | Range | Cooldown | Speed | Acquire | Role |
|---|---|---|---|---|---|---|---|---|---|
| Worker | Light | 40 | 4 | – | melee | 20 | 4 | – (never auto) | economy |
| Soldier | Medium | 70 | 8 | +8 vs Light | melee | 16 | 4 | 160 | beats Rangers: reaches them, bonus damage |
| Ranger | Light | 45 | 7 | +7 vs Heavy | 120 | 24 | 4 | 200 | beats Brutes: shoots them from range |
| Brute | Heavy | 160 | 16 | +8 vs Medium | melee | 30 | 3 | 160 | beats Soldiers: soaks and hits hard |
| Buildings | Structure | 400–1000 | – | – | – | – | – | – | targets |

- "Melee" means range 4: touching plus a little.
- Range is measured **edge to edge**: `dist(centers) <= range + r_attacker + r_target`, compared as `dist_sq` in `i64`.
- Damage = `damage + bonus (if the tag matches)`, at least 1. Armor values come later.
- All numbers are placeholders. The K5 balance test locks only the *direction* of each counter (who wins 5v5), not exact
  numbers.

### Orders: who fights when
| Order | Auto-acquire? | Moves to fight? | After the fight |
|---|---|---|---|
| none (idle) / after `Stop` | yes, within `acquire` | chases up to **leash** 240 from `anchor` | walks back to `anchor` |
| `Move` | **no** (a move is a retreat too) | – | – |
| `AttackMove(pos)` | yes, within `acquire` | yes, no leash | continues to `pos` |
| `Attack(id)` | no, only `id` | chases anywhere | idle (when `id` dies) |
| `Hold` | yes, **in range only** | never | stays |
| `Gather` | no | – | – |

- `anchor` = the position where an idle unit started fighting. It's cleared when the unit gets back.
- `Command::Attack` on an own or allied object is ignored (no friendly fire in the MVP).
- C3 already split Stop and Hold. The difference shows up here: Stop chases, Hold doesn't.

### Target priority (auto-acquire)
Among enemies within `acquire` (Hold: within range), the smallest key wins:

```
( !attacked_me_recently,   // LastHit.by == candidate within the last 40 ticks
  !can_attack,             // armed units before workers and buildings
  dist_sq / (32·32),       // distance in buckets of one cell, so "weakest" can still matter
  hp,                      // weakest
  ObjectId )               // unique tie-break → order-independent
```

- **Sticky:** an auto-acquired target is kept while valid. Every **10 ticks** (`retarget_in`) the key is re-evaluated, and
  the unit switches only if the new best is strictly better on the first two fields. That stops jittery target swaps, but a unit
  still turns on whoever is hitting it. An explicit `Attack(id)` is never re-evaluated.
- A target is invalid if it's dead or gone from the index, or (later, with fog) not visible.

### Systems (tick order)
`store_prev → apply → advance_orders → movement → gather → depletion → build/construct/produce → combat → death`
- **combat:** two-phase, as above. Chase movement happens here with `step_toward`, the same way gather does its own
  walking. `Order::Move` stays in `movement`.
- **death:** units and buildings with `hp == 0`, in `ObjectId` order → `Died { id, owner, pos, building }`, despawn, remove
  from the index.
  - Buildings also clear their grid cells. The production queue is lost with no refund.
  - Node claims and selections need no cleanup: they already treat dead ids as stale.

### Buildings
- `Health` from `BuildingDef.hp` (Base 1000, Barracks 600, Depot 400).
- Under construction: hp starts at 10 % of max and grows with progress. A half-built building is easy to kill.
- No attacking buildings (towers) in the MVP.
- "No buildings left" is a loss condition, which belongs to [08](08-match-flow.md). Combat just emits `Died`.

### Engine or game?
| Part | Where | Why |
|---|---|---|
| `nearest_by` (from `economy::nearest`) | **engine** `phalanx_sim` | 2nd use (gather + targeting), rule of two; generic over the id type |
| `Cooldown` tick timer | **engine** `phalanx_sim` | 2nd use (harvest `ticks_left` + attack cooldown) |
| Two-phase "decide on snapshot, then apply" | game for now | a pattern, not code yet; would become an engine helper if a second system needs it |
| Spatial query for acquire | **engine** later (milestone 5) | brute force is O(n²); fine up to ~200 units, then a spatial hash on top of `Grid` |
| Units, stats, priority, counters | game | Dot Wars rules |

### Events → client
| Event | Client reaction |
|---|---|
| `Attacked { attacker, target, ranged }` | ranged: a 0.1 s tracer line; melee: a short flash on the target |
| `Died { id, owner, pos, building }` | a fading circle or square at `pos`, later a sound |

### Client
- **Health bars** above units and buildings: always when damaged or selected, hidden at full hp. Green → yellow → red.
- **Smart right click:** enemy under the cursor → `Attack` for own units that can attack (workers too). Picking takes both units and
  buildings.
- **`A` attack mode:** `A` + left click on ground → `AttackMove`, `A` + left click on an enemy → `Attack`. Right click or `Esc`
  cancels. Shift (C6) queues.
- **Debug overlay:** the first selected unit's target id, cooldown and anchor.

### Checksum
Add `Health`, `Combat` (cooldown, target, anchor, retarget_in), `LastHit`, and the `UnitKind` (already from P1).

### Testing (headless)
- **Range:** a target one unit outside the range isn't hit; at exactly the range it is (edge-to-edge math).
- **Cooldown:** damage lands exactly every `cooldown` ticks.
- **Mutual kill:** two equal units in melee die on the same tick. **Swapping spawn order gives the same checksums** (fairness).
- **Priority:** a unit prefers the enemy hitting it over a nearer one; prefers armed units over a worker; ties resolve by id.
- **Hold:** never moves, shoots only in range. **Idle:** chases until the leash, then walks back to the anchor. **AttackMove:** fights, then
  reaches the target. **Move:** walks through enemies without stopping.
- **Attack(id):** chases across the map; the target dies → idle next tick. Attack on an own unit → ignored.
- **Death cleanup:** dead objects are gone from the index; a dead building's grid cells are free; a worker whose node claim belonged to a dead
  worker can claim it.
- **Counters (balance smoke test):** 5 Soldiers beat 5 Rangers, 5 Rangers beat 5 Brutes, 5 Brutes beat 5 Soldiers, in a fixed
  layout. Only the winner is asserted, not the margin.
- **Determinism:** a 20v20 brawl for 600 ticks, twice, with equal checksums.

## Steps

| Step | What | Needs | Status |
|---|---|---|---|
| K0 | **Engine:** `nearest_by` and `Cooldown` in `phalanx_sim`; economy switches to them | — (touches `dotwars_sim`: after production's current step) | ✅ |
| K1 | `Health` on units and buildings, `death` system, `Died`, cleanup (index, grid); client health bars and death effect | P1 | ⬜ |
| K2 | Stats in `UnitDef`, per-unit speed, `Command::Attack`, `Order::Attack`, two-phase combat (chase, range, cooldown, hit), `Attacked`; client smart right click on an enemy + tracers | K0, K1 | ⬜ |
| K3 | Auto-acquire: idle/Stop (leash + anchor), Hold (range only), `LastHit`, priority key, sticky retarget | K2 | ⬜ |
| K4 | `Command::AttackMove`, `Order::AttackMove`, `A` attack mode (C7 done with K2 + K4) | K3 | ⬜ |
| K5 | Ranger and Brute in `data.rs` + Barracks trains all three; counter smoke tests; the demo scenario's unit blocks become a mix | K3, P5 | ⬜ |
| K6 | Building hp during construction, building death frees the grid and drops the queue | K1, P2 | ⬜ |
| — | Projectiles in flight, area damage, armor values, abilities, towers, spatial hash for acquire (milestone 5) | K5 | ⬜ later |

K1–K4 don't need training: the demo scenario's two unit blocks (12 vs 8) are already an arena once they get
`UnitKind::Soldier` in P1.

**Done when (K0–K6):** pull a group of Soldiers toward the red block. They engage on their own, health bars drop,
units die and vanish. Hold stays put, Stop chases a bit and returns, A-click walks through and fights.
Rangers shoot (tracers; no kiting yet), and Brutes win against Soldiers.

## Status

**Implemented:** K0. **Next:** K1 (needs P1, which is done). An implementation plan per step comes before K1 starts.
- K0: `phalanx_sim::nearest_by(from, candidates)` (key `(dist_sq, id)`, generic over the id) and `phalanx_sim::Cooldown`
  (`new`, `ready`, `remaining`, `start`, `tick() -> bool` true only on the tick it hits 0), each with unit tests.
  Economy dropped its own `nearest`; `GatherState::Harvesting` holds `timer: Cooldown`. Harvest timing is unchanged
  (the checksum hashes `timer.remaining()`).
