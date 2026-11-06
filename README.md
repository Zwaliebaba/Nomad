# Nomad

*Working title (Design D-018).*

Nomad is a persistent multiplayer space RTS written in Rust. Each commander starts with a mother station and grows an empire of factories, shipyards and fleets. The Universe they share keeps evolving when they log off. Each commander's **Deputy** always executes their standing orders, and while the player is away it keeps the standard actions running and defends their holdings. The look is an isometric voxel diorama with a fixed-pitch camera, 4-way snap rotation and a military HUD.

Targets: one Universe of up to 100 commanders, played in seasons. The MVP is one human player against up to 9 AI commanders, running on a local headless server.

## Status

The MVP design is complete; the build works towards milestone MVP-α, the headless game (Design doc 30). Done: the deterministic simulation core, the Universe structure, ships, fleets and orders (S1–S3), the economy (S4) and ship combat (S5a), the server shell with persistence and catch-up (N1–N2), and phase-1 client milestones M1–M4. Start at [`Design/README.md`](Design/README.md).

## Repository layout

```
Design/                 Design documents (source of truth), including the phase-1 visual slice package
crates/
  nomad-client/         Game client (phase 1: the visual slice; milestones M1–M4 done)
  nomad-sim/            Deterministic simulation (Design doc 11): tick, timers, SimId, random streams, snapshots; galaxy, fleets, economy, combat
  nomad-types/          Shared primitives (Design doc 20 §3): SimId and typed ids, ticks, Universe time, small enums, command shapes
  nomad-protocol/       Wire protocol (Design doc 20 §5): versioned client and server messages, length-prefixed postcard framing
  nomad-harness/        Headless fast-forward runner: pacing runs and determinism checks
  nomad-server/         Server (N2, N3): runs one Universe in real time, saves it to SQLite, recovers, catches up, serves clients over QUIC
  nomad-link/           Client side of the connection (D-125): finds and authenticates to a server, for the client and the tests
.github/                CI workflow, Dependabot, pull request template
tools/preview/          Single-file WebGL preview of the phase-1 scene (design-review aid, not the client)
tools/station-spread/   Rebuilds the phase-1 station layout with the outer buildings spread out (Design D-111)
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

Run the phase-1 client (milestones M1 to M4: window, background, infinite ops grid, camera, the station voxels, bloom on emissive voxels, elevation cues under lifted modules, the hover highlight with its `ELEV +n` tag, and the shield globe with its sector ring, the weak E sector and scripted hit ripples):

```bash
cargo run -p nomad-client                      # borderless fullscreen on the current monitor
cargo run -p nomad-client -- --windowed        # 1920x1080 window
cargo run -p nomad-client -- --size 1280x720   # other window size
cargo run -p nomad-client -- --screenshot out.png --view 1 --zoom 0.5   # one frame offscreen, no window
cargo run -p nomad-client -- --screenshot out.png --hover 700,340       # as if the cursor were at that pixel
cargo run -p nomad-client -- --screenshot out.png --turn-ms 125         # halfway through a +90° turn
cargo run -p nomad-client -- --screenshot out.png --shield-ms 450       # 450 ms into the shield's hit loop
```

| Key | Action |
|---|---|
| Q / E | Rotate the view −90° / +90° (250 ms; presses during a turn are queued) |
| Mouse wheel | Zoom step toward the cursor (0.5, 0.75, 1, 1.5, 2) |
| WASD, arrows, screen edge, middle-mouse drag | Pan (±200 voxels around the deck centre) |
| Home | View 0, zoom 1, deck centre |
| F11 | Toggle between borderless fullscreen and a window |
| F12 | Screenshot to `screenshots/` in the working directory |

Hovering a module highlights it; a lifted module (FA-1, FB-2) also shows its `ELEV +n` tag. The tag needs the phase-1 fonts, which are OFL-licensed and not bundled: put the JetBrains Mono and Rajdhani `.ttf` files (static or variable) into `Design/phase1-visual-slice/assets/fonts/`, which git ignores. Without them the client logs a warning and draws no tag text.

Assets load from `Design/phase1-visual-slice/assets`; set `NOMAD_ASSETS` to use another folder. Tuning values (pan speed, pan limit, grid fade, background and stars, bloom, elevation cues and hover) live in `layout/scene_config.json` there (Design D-059, D-063); the tag's text and style live in `layout/hud_layout.json`. `RUST_LOG` sets the log filter. On Linux the client needs a Vulkan or OpenGL ES driver and, under X11, `libxkbcommon-x11` at run time; building needs no system packages. GPU tests are ignored by default; run them with `cargo test -p nomad-client -- --ignored` on a machine with a GPU or a software Vulkan driver such as Mesa lavapipe.

Run the headless simulation harness (fast-forward, reports the state hash and speed):

```bash
cargo run -p nomad-harness -- --ticks 36000 --diagnostics 10000
cargo run -p nomad-harness -- --fleets --seed 7          # fleet demo: random orders, consistency checks
cargo run -p nomad-harness -- --combat skirmish          # time-to-kill scenarios (also: battle, stress)
cargo run --release -p nomad-harness -- --economy --days 7   # scripted builders, D-070 pacing measures
```

### Run the server

`nomad-server` hosts one Universe without a window. It saves continuously to `<data>/universes/<id>/universe.db`. The data directory is `%LOCALAPPDATA%\Nomad` on Windows and `~/.local/share/Nomad` on Linux; set `NOMAD_DATA` or pass `--data-dir` to use another.

```bash
cargo run -p nomad-server -- create                  # new stage-1 Universe: random seed, 3 AI, Normal, 8 weeks
cargo run -p nomad-server -- create --seed 42 --name "Test" --season-weeks 10
cargo run -p nomad-server -- run                     # run it; Ctrl+C saves and stops
cargo run -p nomad-server -- status                  # meta, last tick, snapshots, journal, without running it
cargo run -p nomad-server -- connect-info            # address, fingerprint, token and an import string for a client
cargo run -p nomad-server -- rotate-token            # new token (stopped server only); --certificate renews the certificate too
```

- `--universe <id>` picks a Universe when the data directory holds more than one. A second `run` on the same Universe is refused.
- **Network (N3):** `run` listens for clients over QUIC on UDP `127.0.0.1:31790`. `--bind`/`NOMAD_BIND` and `--port`/`NOMAD_PORT` change the address and port; port 0 picks any free one (Design D-150). A local client finds the server through `server.json` in the Universe's directory, which holds the address, the certificate fingerprint and the token, and is removed on a clean stop.
- **Identity:** the certificate, its key and the token are made once, at `create`, and kept in `identity.json` beside the database (D-151). Treat both files like a password: on Linux they are readable by your user only; on Windows they rely on your profile's permissions, and the server warns if the data directory lies outside your profile (D-122). `connect-info` prints what a client on another side of a container boundary needs, as a `nomad://` import string; `rotate-token` makes imported details stop working.
- After the server has been stopped, `run` first catches up the missed time at fast-forward speed, then runs live at 10 Hz.
- When the season's final tick has run, the Universe stops advancing: the server saves it and keeps serving that final state until Ctrl+C, and refuses inputs. `run` on an ended season catches up only to the season end, and `status` says the season has ended (Design D-116). A new season is a new Universe.
- Every 60 s of Universe time the journal records a state hash, and a restart checks the replay against it. A mismatch is logged as an error, and the Universe still starts (D-109). The journal is kept for 48 h (D-110).
- `create --backdate-hours N` starts the season N hours in the past, so the first `run` catches up. Use it to test catch-up.
- `RUST_LOG` sets the log filter (default `info` for `run`). Logs go to stderr.

### Run the server in Docker

The `Dockerfile` builds a Linux image of the same server (D-029), which runs in Docker Desktop. In the MVP this is a developer and soak-test mode (D-152). Run these from the repository root:

```bash
docker build --build-arg NOMAD_BUILD_ID=$(git rev-parse HEAD) -t nomad-server .
docker run --rm -v nomad-data:/data nomad-server create --name "Test"
docker run -d --name nomad -v nomad-data:/data -p 127.0.0.1:31790:31790/udp nomad-server
docker exec nomad nomad-server connect-info                    # the import string for the client
docker exec nomad nomad-server status
docker logs -f nomad
docker stop nomad                                              # final snapshot, then exit
```

- `NOMAD_BUILD_ID` is required: the build fails without it.
- **Publish the port on loopback only**, as above (`127.0.0.1:`), so only this computer can connect (D-085, D-150). The image listens on every address inside the container. If you publish it on another host port, pass that port to `connect-info --port`.
- The client cannot read the container's files, so it imports the details from `connect-info` once. They survive restarts and image upgrades (D-151); `docker exec nomad nomad-server rotate-token` works only while the server is stopped, so run it with `docker run --rm -v nomad-data:/data nomad-server rotate-token` after `docker stop`.
- Keep Universes in a named volume, as above. Without `-v` they go to an anonymous volume, and `--rm` deletes them. A named volume lives on Docker Desktop's disk, so resetting Docker Desktop deletes it too.
- **Upgrades:** a journal is replayed only by the build that wrote it (D-048). After an unclean stop, a new image refuses to start. Start the previous image, stop it with `docker stop`, then switch.

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
