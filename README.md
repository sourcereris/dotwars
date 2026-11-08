# Dot Wars

A small learning RTS: squares and circles, two sides, one resource.
Built on the engine in [`../phalanx`](../phalanx), which must sit next to this folder.

```
GameDev/Rust/
  phalanx/   engine (own repo, own workspace)
  dotwars/         this game (own workspace)
```

## Run

```sh
cargo run -p dotwars_client
```

Right click moves the unit.

## Crates

| Crate | Role |
|---|---|
| `dotwars_sim` | game rules, headless (hecs); determinism test |
| `dotwars_client` | the game binary (macroquad) |

See [GAME_DESIGN.md](GAME_DESIGN.md) for the genre foundation, MVP scope and the (open) "soul" of the game.
