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
| Ranger | Light | 45 | 8 | +14 vs Heavy | 150 | 20 | 4 | 220 | beats Brutes: shoots them from range |
| Brute | Heavy | 130 | 16 | +8 vs Medium | melee | 30 | 3 | 160 | beats Soldiers: soaks and hits hard |
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
- **Mutual kill:** two equal units in melee die on the same tick. **Swapping spawn order gives the same death tick** (fairness; checksums differ because the ids swap).
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
| K1 | `Health` on units and buildings, `death` system, `Died`, cleanup (index, grid); client health bars and death effect | P1 | ✅ |
| K2 | Stats in `UnitDef`, per-unit speed, `Command::Attack`, `Order::Attack`, two-phase combat (chase, range, cooldown, hit), `Attacked`; client smart right click on an enemy + tracers | K0, K1 | ✅ |
| K3 | Auto-acquire: idle/Stop (leash + anchor), Hold (range only), `LastHit`, priority key, sticky retarget | K2 | ✅ |
| K4 | `Command::AttackMove`, `Order::AttackMove`, `A` attack mode (C7 done with K2 + K4) | K3 | ✅ |
| K5 | Ranger and Brute in `data.rs` + Barracks trains all three; counter smoke tests; the demo scenario's unit blocks become a mix | K3, P5 | ✅ |
| K6 | Building hp during construction, building death frees the grid and drops the queue | K1, P2 | ✅ |
| — | Projectiles in flight, area damage, armor values, abilities, towers, spatial hash for acquire (milestone 5) | K5 | ⬜ later |

K1–K4 don't need training: the demo scenario's two unit blocks (12 vs 8) are already an arena once they get
`UnitKind::Soldier` in P1.

**Done when (K0–K6):** pull a group of Soldiers toward the red block. They engage on their own, health bars drop,
units die and vanish. Hold stays put, Stop chases a bit and returns, A-click walks through and fights.
Rangers shoot (tracers; no kiting yet), and Brutes win against Soldiers.

## Implementation plan (K1–K2)

Written against the code after P6 + K0. Paths are under `crates/`.

### Decisions (changes from or additions to the design)
- **One spawn path.** `spawn_unit` creates a Soldier and `spawn_worker` patches it into a Worker. K1 replaces both with
  `spawn_kind(kind, pos, owner)`, which reads `data::unit(kind)` for `Body`, `Health` and (K2) `Combat`. Workers also get
  `Worker`. Production (`production.rs:137`) and `add_base` call it. `spawn_unit`/`spawn_worker` stay as thin test wrappers.
- **K1 buildings get full hp, even under construction.** The 10 % → 100 % growth is K6. K1 needs nothing more than
  `BuildingDef.hp`.
- **`UnitDef.hp` arrives in K1, the rest of the stats in K2.** That way each step adds only fields it uses (no dead fields
  under clippy).
- **`Combat` in K2 is `{ cooldown, target }` only.** `anchor` and `retarget_in` come with K3, which is the first step that reads them.
  `LastHit` is K3 too.
- **Each unit either moves or hits on a tick, never both.** A chaser that reaches range this tick hits next tick, which
  keeps the decide phase simple.
- **Cooldown order:** every `Combat.cooldown.tick()` runs first, then decide. A hit calls `start(def.cooldown)`.
  Result: hits land every `cooldown` ticks exactly (hit at t, again at t + cooldown).
- **Buildings as targets are circles.** Their radius is the existing `Body.radius` (half the shorter side). A rectangle
  check is "later" and not worth it in the MVP.
- **Fairness test, corrected:** swapping spawn order swaps the ids, so the checksums *can't* match. The test asserts
  instead that both units die on the same tick, with the same hp traces mirrored.
- **Death needs no new cleanup code.** Construction already handles a vanished site (`site == None`). `nearest_dropoff`
  re-searches on every trip. `supply()` counts live entities, so it drops by itself. `despawn` already frees the grid and
  the index. K1 only proves all of this with tests.

### K1 – Health, death, cleanup
| File | Change |
|---|---|
| `dotwars_sim/src/data.rs` | `UnitDef.hp` (Worker 40, Soldier 70); `BuildingDef.hp` (Base 1000, Barracks 600, Depot 400) |
| `dotwars_sim/src/components.rs` | `Health { hp: u32, max: u32 }` |
| `dotwars_sim/src/lib.rs` | `spawn_kind`; `Health` in `spawn_kind` and `spawn_building`; `Event::Died { id, owner, pos, building }`; `run_death` last in `tick`; checksum hashes `hp, max` |
| `dotwars_sim/src/combat.rs` (new) | `run_death`: walk `sorted_objects()` with `Health.hp == 0`, emit `Died`, then `despawn`. `#[cfg(test)] fn hurt(id, n)` so tests can hurt things before K2 exists |
| `dotwars_sim/src/combat_tests.rs` (new) | tests below |
| `dotwars_client/src/view.rs` | `hp: (u32, u32)` in `UnitView` and `BuildingView` |
| `dotwars_client/src/main.rs` (draw) | health bar above units and buildings when damaged or selected; green above 60 %, yellow above 30 %, red otherwise |
| `dotwars_client/src/effects.rs` | `Died` → a fading circle (unit) or square (building) at `pos` for 0.5 s |
| selection / groups | check that dead ids drop out of the selection and the control groups (they may already be filtered through the views; add a test if they aren't) |

**Tests (sim):**
- At hp 0, the object is gone from the index after the same tick, and exactly one `Died` event comes out, with the right owner and `building` flag.
- Several deaths in one tick: the `Died` events are in `ObjectId` order.
- A dead building's cells are free, and `can_place` passes there again.
- A dead Barracks drops its queue with no refund, and `supply()` drops too.
- A dead drop-off: a carrying worker goes to the next nearest one, or waits if none is left.
- A dead site under construction: its builder goes idle.
- A dead worker's node claim: another worker can claim it.
- A unit killed while it's in a selection command is skipped (as `dead_unit_in_command_is_skipped` does today).
- `Health` changes the checksum.

**Tests (client):** the health-bar color thresholds, the bar hidden at full hp unless selected, and the `Died` effect expiring.

**Done when:** in the game, nothing changes yet unless a test hurts something. The bars and the death effect are in place, so K2
lights them up.

### K2 – Attack
| File | Change |
|---|---|
| `dotwars_sim/src/data.rs` | `Tag { Light, Medium, Heavy, Structure }`; `UnitDef` gets `damage, range: Fixed, cooldown: u16, speed: Fixed, tag, bonus: Option<(Tag, u32)>`; `pub const MELEE: Fixed = 4`. Worker 4 dmg / cd 20, Soldier 8 (+8 vs Light) / cd 16, both at speed 4 and melee range |
| `dotwars_sim/src/economy.rs` | `step_toward(pos, target, stop_at, speed)`. Callers (gather, build, construct) pass `data::unit(kind).speed` |
| `dotwars_sim/src/systems.rs` | `movement` queries `&UnitKind` for speed. The `SPEED` constant goes away |
| `dotwars_sim/src/components.rs` | `Combat { cooldown: Cooldown, target: Option<ObjectId> }`; `Order::Attack(ObjectId)` |
| `dotwars_sim/src/lib.rs` | `Command::Attack { units, target, queue }`. `apply` ignores it if the target is missing or on the same team, and gives the order only to own units with `Combat`. `hash_order` gets a new tag; the checksum hashes `Combat`. `Event::Attacked { attacker, target, ranged }`. `run_combat` goes before `run_death` |
| `dotwars_sim/src/combat.rs` | `run_combat` (below) and `fn damage(att: &UnitDef, target_tag) -> u32` |

`run_combat`, one tick:
1. **Tick cooldowns** for every `Combat`.
2. **Snapshot:** a `BTreeMap<ObjectId, Snap { pos, radius, team, tag }>` of everything with `Health`.
3. **Decide** (`sorted_units()` with `Order::Attack(t)`), into a `Vec<(ObjectId, Intent)>`:
   - `t` is not in the snapshot → `Intent::Done`.
   - In range (`dist_sq <= (range + r_a + r_t)²`, in `i64`) and the cooldown is ready → `Intent::Hit(t, dmg)`.
   - In range, cooldown not ready → `Intent::Wait`.
   - Out of range → `Intent::Chase(t's pos)`.
4. **Apply**, in this order:
   - Every `Chase` does `step_toward(pos, target, range + r_a + r_t, speed)`.
   - Every `Done` sets `orders.current = None`, so `advance_orders` takes the next order next tick.
   - Every `Hit` does `hp = hp.saturating_sub(dmg)`, then `cooldown.start(def.cooldown)`, `combat.target = Some(t)`, and emits
     `Attacked { ranged: def.range > MELEE }`.

Then `run_death` removes the dead. Their attackers see `Done` on the next tick.

| File (client) | Change |
|---|---|
| `dotwars_client/src/orders.rs` | `pick_enemy(units, buildings, pos, tolerance)`: an enemy unit by radius + tolerance, or else an enemy building by `contains`. `right_click_commands` checks it **first**: on a hit, all selected own units get `Command::Attack` (workers too) |
| `dotwars_client/src/effects.rs` | `Attacked`: ranged → a 0.1 s line from attacker to target; melee → a 0.15 s white flash ring on the target. Positions come from the current views, and the effect is skipped if either id is gone |

**Tests (sim):**
- **Range:** a target 1 unit past `range + r_a + r_t` takes no damage; at exactly that distance it does.
- **Cooldown:** hp drops every 16 ticks for a Soldier, and nothing lands in between.
- **Chase:** an Attack on a far target walks there and hits. If the target moves, the attacker follows.
- **Mutual kill:** two Soldiers die on the same tick. With spawn order swapped, both still die on that same tick.
- **Bonus:** a Soldier hits a Worker for 16 and a Soldier for 8.
- **Ignored:** an Attack on an own unit or a missing id changes nothing (equal checksums with and without the command).
- **Target dies:** the attacker is idle on the next tick. A queued `Move` after the `Attack` starts then.
- **Speed:** all units are at speed 4, so existing movement and economy tests keep their checksums.
- **Determinism:** a 10v10 Attack brawl over 400 ticks, run twice, gives equal checksums.

**Tests (client):** a right click on an enemy unit or building → `Attack` for everyone selected; a right click on an own unit → `Move`; tracers expire.

**Done when:** select the blue soldiers and right-click a red one. They walk over, hit, the health bars drop, and the target dies
and vanishes. A right click on a red building wears it down until it dies and frees its cells.

## Status

**Implemented:** K0–K6 (sim and client; needs a human check in the game). **Next:** [05 Map](05-map.md) or [07 AI](07-ai.md).
- Sim: `combat.rs` (`run_combat`, `run_death`, `damage`), `combat_tests.rs` (31 tests). Components `Health`, `Combat`, `LastHit`;
  orders `Attack`, `AttackMove`; commands `Attack`, `AttackMove`; events `Attacked`, `Died`. `spawn_kind` is the one spawn path.
  `step_toward` takes a speed, and `UnitDef.speed` replaced the global `SPEED`.
- Client: health bars (damaged or selected), smart right click on an enemy, `A` attack mode (Shift queues and stays in the mode),
  `Q`/`W`/`E` train slots, tracers/flashes/death bursts, a combat line in the F1 overlay.
- Differences from the design:
  - Balance numbers moved so the counter smoke tests hold without kiting: Ranger 8 dmg (+14 vs Heavy), range 150, cooldown 20,
    acquire 220; Brute 130 hp. The tests lock only the winner of each 5v5.
  - Building hp grows with a gain per construction tick and ignores buildings already at 0 hp (so a dead site can't be "healed").
  - `LastHit` is written only on objects that have `Combat`.
  - Client effects need the target to be alive when the frame draws, so the tracer of a mutual kill is skipped.
- Not done: the demo scenario is still 2 soldiers per player (supply cap 10); mixed blocks for a bigger arena come with the AI or
  map work. No hand playtest yet: check right click attack, `A`, Hold vs Stop, bars, and training Rangers/Brutes with `W`/`E`.
