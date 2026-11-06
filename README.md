# Nomad

*Working title (Design D-018).*

Nomad is a persistent multiplayer space RTS written in Rust. Each commander starts with a mother station and grows an empire of factories, shipyards and fleets. The Universe they share keeps evolving when they log off. Each commander's **Deputy** always executes their standing orders, and while the player is away it keeps the standard actions running and defends their holdings. The look is an isometric voxel diorama with a fixed-pitch camera, 4-way snap rotation and a military HUD.

Targets: one Universe of up to 100 commanders, played in seasons. The MVP is one human player against up to 9 AI commanders, running on a local headless server.

## Status

Design and build run in parallel (Design D-051). Being built: the phase-1 visual slice (`nomad-client`) and the deterministic simulation core (`nomad-sim`), still without game rules. Start at [`Design/README.md`](Design/README.md).

## Repository layout

```
Design/                 Design documents (source of truth), including the phase-1 visual slice package
crates/
  nomad-client/         Game client (phase 1: the visual slice; milestones M1–M2 done)
  nomad-sim/            Deterministic simulation core (Design doc 11): tick, timers, SimId, random streams, snapshots
  nomad-types/          Shared primitives (Design doc 20 §3): SimId and typed ids, ticks, Universe time, small enums, command shapes
  nomad-protocol/       Wire protocol (Design doc 20 §5): versioned client and server messages, length-prefixed postcard framing
  nomad-harness/        Headless fast-forward runner: pacing runs and determinism checks
  nomad-server/         Server shell (N2 plan): runs one Universe in real time, saves it to SQLite, recovers and catches up
.github/                CI workflow, Dependabot, pull request template
tools/preview/          Single-file WebGL preview of the phase-1 scene (design-review aid, not the client)
AGENTS.md               Instructions for AI coding agents
CONTRIBUTING.md         How to propose and land changes
CODING_STANDARDS.md     Rust coding standards
```

## Building

You need [rustup](https://rustup.rs). The toolchain version pinned in `rust-toolchain.toml` installs automatically on the first `cargo` command.

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Run the phase-1 client (milestones M1 to M3: window, background, infinite ops grid, camera, the station voxels, bloom on emissive voxels, elevation cues under lifted modules, and the hover highlight with its `ELEV +n` tag):

```bash
cargo run -p nomad-client                      # 1920x1080 window
cargo run -p nomad-client -- --size 1280x720   # other window size
cargo run -p nomad-client -- --screenshot out.png --view 1 --zoom 0.5   # one frame offscreen, no window
cargo run -p nomad-client -- --screenshot out.png --hover 700,340       # as if the cursor were at that pixel
cargo run -p nomad-client -- --screenshot out.png --turn-ms 125         # halfway through a +90° turn
```

| Key | Action |
|---|---|
| Q / E | Rotate the view −90° / +90° (250 ms; presses during a turn are queued) |
| Mouse wheel | Zoom step toward the cursor (0.5, 0.75, 1, 1.5, 2) |
| WASD, arrows, screen edge, middle-mouse drag | Pan (±200 voxels around the deck centre) |
| Home | View 0, zoom 1, deck centre |
| F11 | Toggle fullscreen |
| F12 | Screenshot to `screenshots/` in the working directory |

Hovering a module highlights it; a lifted module (FA-1, FB-2) also shows its `ELEV +n` tag. The tag needs the phase-1 fonts, which are OFL-licensed and not bundled: put the JetBrains Mono and Rajdhani `.ttf` files (static or variable) into `Design/phase1-visual-slice/assets/fonts/`, which git ignores. Without them the client logs a warning and draws no tag text.

Assets load from `Design/phase1-visual-slice/assets`; set `NOMAD_ASSETS` to use another folder. Tuning values (pan speed, pan limit, grid fade, background and stars, bloom, elevation cues and hover) live in `layout/scene_config.json` there (Design D-059, D-063); the tag's text and style live in `layout/hud_layout.json`. `RUST_LOG` sets the log filter. On Linux the client needs a Vulkan or OpenGL ES driver and, under X11, `libxkbcommon-x11` at run time; building needs no system packages. GPU tests are ignored by default; run them with `cargo test -p nomad-client -- --ignored` on a machine with a GPU or a software Vulkan driver such as Mesa lavapipe.

Run the headless simulation harness (fast-forward, reports the state hash and speed):

```bash
cargo run -p nomad-harness -- --ticks 36000 --diagnostics 10000
```

### Run the server

`nomad-server` hosts one Universe without a window (N2: no networking yet, so no client connects to it). It saves continuously to `<data>/universes/<id>/universe.db`. The data directory is `%LOCALAPPDATA%\Nomad` on Windows and `~/.local/share/Nomad` on Linux; set `NOMAD_DATA` or pass `--data-dir` to use another.

```bash
cargo run -p nomad-server -- create                  # new stage-1 Universe: random seed, 3 AI, Normal, 8 weeks
cargo run -p nomad-server -- create --seed 42 --name "Test" --season-weeks 10
cargo run -p nomad-server -- run                     # run it; Ctrl+C saves and stops
cargo run -p nomad-server -- status                  # meta, last tick, snapshots, journal, without running it
```

- `--universe <id>` picks a Universe when the data directory holds more than one. A second `run` on the same Universe is refused.
- After the server has been stopped, `run` first catches up the missed time at fast-forward speed, then runs live at 10 Hz.
- `create --backdate-hours N` starts the season N hours in the past, so the first `run` catches up. Use it to test catch-up.
- `RUST_LOG` sets the log filter (default `info` for `run`). Logs go to stderr.

## Continuous integration

GitHub Actions (`.github/workflows/ci.yml`) runs on every push:

- `rustfmt` check
- `clippy` with warnings treated as errors
- build and test on Linux and Windows

CI builds the **Debug** profile only.

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`CODING_STANDARDS.md`](CODING_STANDARDS.md). AI agents follow [`AGENTS.md`](AGENTS.md).

## License

Proprietary. All rights reserved (Design D-031).
