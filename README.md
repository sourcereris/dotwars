# Dot Wars

A small learning RTS: squares and circles, two sides, one resource.
Built on the engine in [`../phalanx`](../phalanx), which must sit next to this folder.

```
GameDev/Rust/
  phalanx/   engine (own repo, own workspace)
  dotwars/   this game (own repo, own workspace)
```

## Run

```sh
cargo run -p dotwars_client
```

| Input | Action |
|---|---|
| Left click / drag | select unit / box-select own units (Shift adds or toggles) |
| Right click | move selected own units |
| `S` / `H` | stop / hold position |
| `Ctrl+N` / `Shift+N` / `N` | assign / add to / select control group N (tap `N` twice to center camera) |
| Arrows, window edges, middle mouse | pan camera |
| Mouse wheel | zoom toward cursor |
| `F1` | toggle debug overlay |

## Crates

| Crate | Role |
|---|---|
| `dotwars_sim` | game rules, headless (hecs); determinism test |
| `dotwars_client` | the game binary (macroquad) |

See [GAME_DESIGN.md](GAME_DESIGN.md) for the genre foundation and MVP, and [docs/](docs/README.md) for per-system design, steps and implementation status.
