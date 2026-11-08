# Dot Wars – Game Design

Dot Wars is built in two layers:

1. **Foundation:** the generic RTS mechanics every game in the genre needs. They're described here
   without game-specific details (no unit names, numbers or factions). This is the actual goal of the project.
2. **Soul:** what would make Dot Wars different from other RTS games. It's left open on purpose
   (see the last section). It's an idea, not a target, and nothing here waits for it.

Each feature is tagged:
- **MVP:** needed for the first complete, playable match against an AI.
- **Later:** improves the game, but the MVP works without it.
- **Where:** `engine` = generic, a candidate for Phalanx (rule of two still applies: build it in the game first, move it once a second use appears); `game` = always Dot Wars.

---

## The three loops

Every RTS is three loops feeding each other:

```
 Economy ──► Production ──► Combat
    ▲                          │
    └──── map control ◄────────┘
```

- **Economy:** gather resources.
- **Production:** turn resources into buildings, units and upgrades.
- **Combat:** use units to win fights, take map control and deny the enemy's economy.

Every mechanic below supports one of these loops or the player's ability to control them.

---

## 1. Control

| Feature | Tag | Where |
|---|---|---|
| Select one (click), many (box drag), add/remove (Shift) | MVP | engine (input), game (rules) |
| Select all of a type on screen (double click) | Later | game |
| Move, stop, hold position | MVP | game |
| Attack target, **attack-move** (move but engage anything met) | MVP | game |
| Command queue (Shift + order) | Later | game |
| Patrol | Later | game |
| Control groups (Ctrl+1..9) and jump camera to group | MVP | engine (input), game |
| Rally points for production buildings | Later | game |
| Group movement / formations (group moves together, keeps speed of slowest) | Later | engine (steering) |
| Rebindable hotkeys | Later | engine |

**Principles**
- Every command gives **instant feedback** (marker, sound, selection flash), even though the sim acts after the input delay.
- Commands are the only way to change the game (see ARCHITECTURE.md). The AI and replays use the same commands.

## 2. Economy

| Feature | Tag | Where |
|---|---|---|
| Resource nodes on the map, finite amount | MVP | game |
| Workers gather and return to a drop-off building | MVP | game |
| Worker crowding handled (spread across nodes, no deadlocks) | MVP | game |
| Player resource stockpile | MVP | game |
| Supply / population cap, raised by a building | Later | game |
| Second resource type | Later | game |
| Expansion: new drop-off near distant resources | Later | game |

**Principles**
- Resources run out, which pushes players to expand and move across the map.
- Spending is always a choice: economy now vs. army now.

## 3. Production

| Feature | Tag | Where |
|---|---|---|
| Building placement on the grid, validity check, preview | MVP | engine (grid), game (rules) |
| Construction over time by a worker | MVP | game |
| Unit production queue in a building, cancel/refund | MVP | game |
| Tech requirements (building A unlocks B) | Later | game |
| Upgrades (+damage, +armor, faster gathering) | Later | game |
| Unit and building stats in data files (`ron`) | Later | engine (loading), game (data) |

**Principles**
- Tech gives a match a time structure: early, mid and late game feel different.

## 4. Combat

| Feature | Tag | Where |
|---|---|---|
| Health, damage, attack range, attack cooldown | MVP | game |
| Melee and ranged units | MVP | game |
| Auto-targeting with clear priority (attacker of me > nearest > weakest) | MVP | game |
| Death, cleanup, dead-target checks | MVP | game |
| Counters between unit types (rock-paper-scissors) | MVP | game |
| Armor types and bonus damage | Later | game |
| Projectiles in flight (not instant hit) | Later | engine (motion), game |
| Area damage | Later | game |
| Active abilities with cooldowns | Later | game |

**Principles**
- No single unit type should win on its own. Counters make army composition a decision.
- A fight should be readable: who's shooting whom, who's about to die.

## 5. Map and information

| Feature | Tag | Where |
|---|---|---|
| Tile grid with passable/blocked cells | MVP | engine |
| Pathfinding (A*, later flow fields), unit avoidance | MVP | engine |
| Fog of war: unexplored / seen before / visible now | MVP | engine (visibility grid), game (vision ranges) |
| Minimap: terrain, units, click to move camera | MVP | engine |
| Map file with gameplay and visual layers | Later | engine |
| Chokepoints, open areas, multiple expansions per map | Later | game (map design) |
| High ground / terrain affecting vision or damage | Later | game |

**Principles**
- Information is a resource. Scouting has to be worth the risk.
- Map layout creates strategy: where to expand, where to defend, where to attack.

## 6. Interface

| Feature | Tag | Where |
|---|---|---|
| RTS camera: edge scroll, keys, drag, zoom | MVP | engine |
| Resource and supply display | MVP | game |
| Selection panel: portrait/icon, health, current action | MVP | game |
| Command card: buttons with hotkeys | MVP | engine (widget), game (contents) |
| Health bars, selection circles, team colors | MVP | engine (drawing), game |
| Alerts: under attack, construction done, not enough resources | Later | game |
| Pause and game speed (single player) | Later | engine |
| Debug overlay: tick time, unit count, checksum, grid, paths | MVP | engine |

## 7. Opponent

| Feature | Tag | Where |
|---|---|---|
| AI that gathers, builds from a script and attacks in waves | MVP | game |
| AI issues the same Commands as a player (no cheating through the back door) | MVP | game |
| Difficulty levels (start: resource bonus) | Later | game |
| AI that scouts and reacts to the player's army | Later | game |

## 8. Match flow

| Feature | Tag | Where |
|---|---|---|
| Start: map, players, starting units and buildings | MVP | game |
| Win/lose: all enemy buildings destroyed | MVP | game |
| Post-game stats (resources, army size over time) | Later | game |
| Replays (seed + command log) | Later | engine |
| Save/load | Later | engine |
| Multiplayer lobby, map selection | Later | engine (net), game (UI) |
| Campaign / scenarios | Later | game |

---

## What makes it fun (checklist for every feature)

1. **Responsive:** input gets an instant reaction, even before the sim acts.
2. **Pathing that doesn't annoy:** units don't get stuck, circle or block each other.
3. **Readable:** at a glance, you can tell who owns what, what's hurt and what's busy.
4. **Real decisions:** expand or build army? Attack or defend? If the answer is always the same, it's a calculator, not a strategy game.

---

## MVP: first playable match

The smallest complete game: one map, one faction mirrored for both sides, one resource, against an AI.

- Worker, 2–3 combat unit types with counters between them
- Main building (produces workers, drop-off), one production building
- Gather → build → train → fight → destroy all enemy buildings
- Fog of war, minimap, basic HUD, scripted AI

Mapping to engine milestones (ARCHITECTURE.md):

| Engine milestone | Unlocks in the game |
|---|---|
| 3. Commands, selection | Control (select, move) |
| 4–5. Many units, separation | Armies that don't overlap |
| 6. Pathfinding | Units move around obstacles and buildings |
| 7. Combat and economy | Economy, production, combat, win/lose: **MVP playable** |
| 8–9. Lockstep, networking | Multiplayer |
| R1–R6. 2.5D rendering | Models, terrain, animations |

---

## Soul (idea, not a target)

This is where Dot Wars would become *its own* game instead of "an RTS". It's **intentionally left open**.
The project doesn't depend on it: the foundation above is the goal, and it's a complete game without a soul.

If an idea comes, it goes here as a note, not a plan. Directions to think about someday:

- **A twist on one of the three loops:** an unusual economy, production, or a different way to win fights.
- **Theme and style:** what the shapes *are* and why they fight.
- **Scale and pace:** a few units that matter, or huge armies; short matches or long ones.
- **One mechanic** nobody else has, which everything else is built around.

_Notes:_

- _(empty)_
