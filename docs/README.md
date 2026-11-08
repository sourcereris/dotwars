# Dot Wars – Implementation docs

One file per foundation system from [GAME_DESIGN.md](../GAME_DESIGN.md). Each file holds:
- **Design:** how it's built (decided before coding).
- **Steps:** small, ordered implementation steps.
- **Status:** what's actually implemented.

GAME_DESIGN.md says *what* the game needs. These files say *how*, and *how far along* it is.
Engine-side docs live in `../phalanx/docs/` (e.g. [input](../../phalanx/docs/input.md)).

Status legend: ✅ done · 🚧 in progress · ⬜ planned · 📝 design not written yet

| # | System | Design | Progress |
|---|---|---|---|
| 01 | [Control](01-control.md) | ✅ written | 🚧 C0–C3, C5 done; next C4, C6, C7 |
| 02 | [Economy](02-economy.md) | ✅ written + implementation plan | ✅ E0–E5 (needs a human check) |
| 03 | [Production](03-production.md) | ✅ written | ✅ P0–P6 (needs a human check) |
| 04 | [Combat](04-combat.md) | ✅ written | ✅ K0–K6 (needs a human check) |
| 05 | [Map and information](05-map.md) | 📝 | ⬜ |
| 06 | [Interface](06-interface.md) | 📝 | ⬜ |
| 07 | [Opponent (AI)](07-ai.md) | 📝 | ⬜ |
| 08 | [Match flow](08-match-flow.md) | 📝 | ⬜ |

**Workflow:** write the design section before starting a system → implement step by step →
update the Status table in the same commit as the code.
