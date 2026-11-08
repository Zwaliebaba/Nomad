# Nomad

*Working title (Design D-018).*

Nomad is a persistent multiplayer space RTS written in Rust. Each commander starts with a mother station and grows an empire of factories, shipyards and fleets. The Universe they share keeps evolving when they log off. Each commander's **Deputy** always executes their standing orders, and while the player is away it keeps the standard actions running and defends their holdings. The look is an isometric voxel diorama with a fixed-pitch camera, 4-way snap rotation and a military HUD.

Targets: one Universe of up to 100 commanders, played in seasons. The MVP is one human player against up to 9 AI commanders, running on a local headless server.

## Status

The MVP design is complete; the build works towards milestone MVP-α, the headless game (Design doc 30). Done:

- **Simulation:** the deterministic core, the Universe structure, ships, fleets and orders (S1–S3); the economy (S4); ship and station combat (S5a, S5b); commander records, presence and exposure windows (S6a); season standings, the early win and the season end (S8).
- **Server:** shared types and the wire protocol (N1); the server shell with persistence, recovery and catch-up (N2); QUIC transport and sessions, with the `nomad-link` client side (N3).
- **Client:** phase-1 milestones M1–M4.

Next: the siege rules (S6b), sensors (S7) and the Deputy (A1), then the Strategist (A2) and the raider director (A3). Design doc 30 §2 tracks the status; start at [`Design/README.md`](Design/README.md).

## Repository layout

```
Design/                 Design documents (source of truth), including the phase-1 visual slice package
crates/
  nomad-client/         Game client (phase 1: the visual slice; milestones M1–M4 done)
  nomad-sim/            Deterministic simulation (Design doc 11): tick, timers, SimId, random streams, snapshots; galaxy,
                        fleets, economy, ship and station combat, commanders and exposure windows, season standings
  nomad-types/          Shared primitives (Design doc 20 §3): SimId and typed ids, ticks, Universe time, small enums, command shapes
  nomad-protocol/       Wire protocol (Design doc 20 §5): versioned client and server messages, length-prefixed postcard framing
  nomad-harness/        Headless fast-forward runner: pacing runs, scenario runs and determinism checks
  nomad-server/         Server (N2, N3): runs one Universe in real time, saves it to SQLite, recovers, catches up, serves clients over QUIC
  nomad-link/           Client side of the connection (D-125): finds and authenticates to a server, for the client and the tests
                        (nomad-ai, all AI logic, is planned with A1: Design D-199)
.github/                CI workflow, Dependabot, pull request template
.vscode/                Tasks, launch configurations and settings for VS Code
Dockerfile              Linux image of nomad-server, a developer and soak-test mode (Design D-152)
tools/preview/          Single-file WebGL preview of the phase-1 scene (design-review aid, not the client)
tools/station-scene/    Builds and validates the supported SpaceStation assembly (D-290)
art/scenes/SpaceStation/ Authored layout, MagicaVoxel assembly and default renderer assets
AGENTS.md               Instructions for AI coding agents
CONTRIBUTING.md         How to propose and land changes
CODING_STANDARDS.md     Rust coding standards
```

## Building

You need [rustup](https://rustup.rs). The toolchain version pinned in `rust-toolchain.toml` installs automatically on the first `cargo` command.

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
```

### Tests

`cargo test --workspace` runs the fast tests, the same ones CI runs: unit tests and short integration tests, including the determinism and snapshot-stability checks (Design D-049, D-190). Three kinds of test are `#[ignore]`, each with its reason:

- **network tests:** anything that opens a socket, including the client–server integration test;
- **long simulations:** multi-hour catch-up, the full checkpoint replay and the longer harness scenarios;
- **GPU tests:** golden images and offscreen renders of the client.

Run them locally before merging work that touches the server, the network or long-run simulation behaviour, and on Windows as well as Linux for network code (D-190):

```bash
cargo test --workspace -- --ignored    # every ignored test; the GPU tests need a GPU adapter or a software Vulkan driver
cargo test -p nomad-client -- --ignored  # only the GPU tests, after changing render code
```

In VS Code the `Local checks`, `GPU tests` and `Ignored tests (network, long simulations, GPU)` tasks run these.

### Run the client

Run the native renderer with the supported SpaceStation assembly: PascalCase modules, enclosed tubes and four external shield generators (D-290). The older `start`/`full` and phase-1 authoring layouts are retired.

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
| WASD, arrows, screen edge, middle-mouse drag | Pan (within the scene-configured ops area) |
| Home | View 0, zoom 1, deck centre |
| F11 | Toggle between borderless fullscreen and a window |
| F12 | Screenshot to `screenshots/` in the working directory |

These are the phase-1 bindings (phase-1 doc 02). The MVP hotkey scheme (Design D-093, doc 17 §7: rotation on Alt+Q/E, no WASD panning) replaces them with package U7.

Hovering a module highlights it. Lifted modules can show an `ELEV +n` tag. Put the optional OFL-licensed JetBrains Mono and Rajdhani `.ttf` files into `art/scenes/SpaceStation/assets/fonts/`, which git ignores. Without them the renderer logs a warning and omits tag text.

Assets default to `art/scenes/SpaceStation/assets`; set `NOMAD_ASSETS` to use another folder. Tuning values (pan speed, pan limit, grid fade, background and stars, bloom, elevation cues and hover) live in `layout/scene_config.json` there (Design D-059, D-063); the tag's text and style live in `layout/hud_layout.json`. `RUST_LOG` sets the log filter. wgpu picks the graphics backend itself; `WGPU_BACKEND` overrides it, for example `dx12` (D-084). On Linux the client needs a Vulkan or OpenGL ES driver and, under X11, `libxkbcommon-x11` at run time; building needs no system packages. GPU tests are ignored by default; run them with `cargo test -p nomad-client -- --ignored` on a machine with a GPU or a software Vulkan driver such as Mesa lavapipe.

Rebuild the supported layout and preview:

```bash
cargo run --locked -p nomad-vox -- import
python3 tools/vox/audit_modules.py
python3 tools/station-scene/build.py
python3 tools/preview/build.py art/scenes/SpaceStation/assets tools/preview/template.html target/SpaceStation-preview.html
```

Edit [layout.json](art/scenes/SpaceStation/layout.json) for placements and tube routes. The module catalog is generated at `art/voxels/modules/ModuleCatalog.json`. See [the scene documentation](art/scenes/SpaceStation/README.md). Frozen phase-1 data remains for regression tests and shared rendering templates, not as a supported layout.

### Run the harness

`nomad-harness` runs the simulation headless, at fast-forward speed. Without a mode it runs diagnostic timers for one hour of Universe time and reports the state hash and the speed:

```bash
cargo run -p nomad-harness -- --ticks 36000 --diagnostics 10000
```

| Mode | What it runs | Options (defaults) |
|---|---|---|
| *(none)* | Diagnostic timers; the state hash and the speed against real time | `--ticks N` (36000), `--diagnostics N` (10000), `--mean-interval-secs N` (10), `--restore-at N` |
| `--galaxy` | Generates the stage-1 sector and prints a summary: systems, sites, lanes, trip times, deposit values, control (doc 10 §4.4) | — |
| `--fleets` | The fleet demo: the four start kits under scripted and seeded random orders, with consistency checks and tick times (S3) | `--hours N` (6) or `--ticks N`, `--order-interval-secs N` (60), `--restore-at N` |
| `--combat SET` | Time-to-kill scenarios against D-058: `skirmish`, `battle`, `stress` (ship combat), `breach`, `core` (station combat, measured as D-147 says) | — |
| `--economy` | Scripted builder commanders, reporting the D-070 pacing measures, core-kit prices (D-189) and the diminishing-yield factor (D-217) | `--days N` (7) |
| `--windows` | Exposure windows, presence and the lapse of orders for four scripted commanders, checking D-155 on every tick | `--days N` (30) |
| `--season` | A scripted season to its result: control and standings changes, the near-win warning, the result, checked against a recount (S8) | `--days N` (14), `--claim-hours N` (12), `--early-win`, `--restore-at N` |

Every mode takes `--seed N` (default 1) and `--rules PATH`, which replaces the embedded rules file for a tuning run (D-104). `--restore-at N` saves and restores a snapshot at tick N, which must not change the result or the hash. `--help` prints the usage. The long modes (`--economy`, `--windows`, `--season`) run much faster in a release build, for example `cargo run --release -p nomad-harness -- --economy --days 7`; CI itself builds Debug only (D-010).

### Run the server

`nomad-server` hosts one Universe without a window. It saves continuously to `<data>/universes/<id>/universe.db`. The data directory is `%LOCALAPPDATA%\Nomad` on Windows and `~/.local/share/Nomad` on Linux; set `NOMAD_DATA` or pass `--data-dir PATH` (every command takes it) to use another.

```bash
cargo run -p nomad-server -- create                  # new stage-1 Universe: random seed, 3 AI, Normal, 8 weeks, window at local 19:00
cargo run -p nomad-server -- create --seed 42 --name "Test" --season-weeks 10 --window 20:00
cargo run -p nomad-server -- run                     # run it; Ctrl+C saves and stops
cargo run -p nomad-server -- run --bind 127.0.0.1 --port 31790
cargo run -p nomad-server -- status                  # meta, last tick, snapshots, journal, standings and result, without running it
cargo run -p nomad-server -- connect-info            # address, fingerprint, token and an import string for a client
cargo run -p nomad-server -- rotate-token            # new token (stopped server only); --certificate renews the certificate too
cargo run -p nomad-server -- --help                  # every command and option
```

| Command | Options |
|---|---|
| `create` | `--seed N` (random), `--ai N` (3, the only number stage 1 supports), `--difficulty easy\|normal\|hard` (normal, for every AI commander), `--season-weeks 8..12` (8, D-106), `--name TEXT`, `--backdate-hours N`, `--window HH:MM[Z\|±hh:mm]` (local 19:00) |
| `run` | `--universe ID`, `--bind ADDR`, `--port N` |
| `status` | `--universe ID` |
| `connect-info` | `--universe ID`, `--host ADDR`, `--port N` |
| `rotate-token` | `--universe ID`, `--certificate` |

- `--universe <id>` picks a Universe when the data directory holds more than one. A second `run` on the same Universe is refused.
- **Network (N3):** `run` listens for clients over QUIC on UDP `127.0.0.1:31790`. `--bind`/`NOMAD_BIND` and `--port`/`NOMAD_PORT` change the address and port; port 0 picks any free one (Design D-150). A local client finds the server through `server.json` in the Universe's directory, which holds the address, the certificate fingerprint and the token, and is removed on a clean stop.
- **Identity:** the certificate, its key and the token are made once, at `create`, and kept in `identity.json` beside the database (D-151). Treat both files like a password: on Linux they are readable by your user only; on Windows they rely on your profile's permissions, and the server warns if the data directory lies outside your profile (D-122). `connect-info` prints what a client on another side of a container boundary needs, as a `nomad://` import string; `--host` and `--port` change the address it gives. `rotate-token` makes imported details stop working.
- **Catch-up:** after the server has been stopped, `run` first catches up the missed time at fast-forward speed, then runs live at 10 Hz. A running server that falls more than 1 minute behind UTC, for example after the computer slept, catches up the same way (D-117). Every commander counts as offline during catch-up, and commands are refused until the server is live (D-047).
- **Season end:** the season ends at its final tick, or earlier on the tick a commander controls 60% of the claimable systems (6 of 9; D-192). The Universe then stops advancing: the server saves it and keeps serving that final state until Ctrl+C, and refuses inputs (Design D-116). `run` on an ended season catches up only to the season end, and `status` shows the standings and the result (D-196). A new season is a new Universe.
- Every 60 s of Universe time the journal records a state hash, and a restart checks the replay against it. A mismatch is logged as an error, and the Universe still starts (D-109). The journal is kept for 48 h (D-110).
- `create --backdate-hours N` starts the season N hours in the past, so the first `run` catches up. Use it to test catch-up.
- `create --window HH:MM` sets where your daily exposure window starts, in local time; `HH:MMZ` gives it in UTC and `HH:MM+hh:mm` with an offset, for a server whose time zone differs from yours (a container). The default is local 19:00, and the start must fall on a quarter hour in UTC (Design D-155, D-157). From then on your activity moves the window, at most 1 h a day and only when it closes (D-032, D-155, D-226). During your first 7 days a client may also choose a new start; it applies at the next close, in place of that day's move (D-157, D-226).
- `RUST_LOG` sets the log filter (default `info` for `run`, `warn` for the other commands). Logs go to stderr.

### Run the server in Docker

The `Dockerfile` builds a Linux image of the same server (D-029), which runs in Docker Desktop. In the MVP this is a developer and soak-test mode (D-152). Run these from the repository root:

```bash
docker build --build-arg NOMAD_BUILD_ID=$(git rev-parse HEAD) -t nomad-server .
docker run --rm -v nomad-data:/data nomad-server create --name "Test" --window 19:00+01:00
docker run -d --name nomad -v nomad-data:/data -p 127.0.0.1:31790:31790/udp nomad-server
docker exec nomad nomad-server connect-info                    # the import string for the client
docker exec nomad nomad-server status
docker logs -f nomad
docker stop nomad                                              # final snapshot, then exit
```

- `NOMAD_BUILD_ID` is required: the build fails without it. The image holds a Debug build, the profile CI tests (D-010).
- The container does not know your time zone, so give `create --window` your UTC offset, as above, or a UTC time with `Z` (D-157).
- **Publish the port on loopback only**, as above (`127.0.0.1:`), so only this computer can connect (D-085, D-150). The image listens on every address inside the container. If you publish it on another host port, pass that port to `connect-info --port`.
- The client cannot read the container's files, so it imports the details from `connect-info` once. They survive restarts and image upgrades (D-151); `docker exec nomad nomad-server rotate-token` works only while the server is stopped, so run it with `docker run --rm -v nomad-data:/data nomad-server rotate-token` after `docker stop`.
- Keep Universes in a named volume, as above. Without `-v` they go to an anonymous volume, and `--rm` deletes them. A named volume lives on Docker Desktop's disk, so resetting Docker Desktop deletes it too.
- **Upgrades:** a journal is replayed only by the build that wrote it (D-048). After an unclean stop, a new image refuses to start. Start the previous image, stop it with `docker stop`, then switch.

## Continuous integration

GitHub Actions (`.github/workflows/ci.yml`) runs on every push to any branch, and on pull requests from forks:

- `rustfmt` check
- `cargo deny`: licences and advisories (`deny.toml`, D-031)
- on Linux and Windows (D-011): `clippy` with warnings treated as errors, then build and test, all with `--locked`

CI builds the **Debug** profile only (D-010), and runs only the fast tests (D-190). Network tests, long simulations and GPU tests are `#[ignore]` and run locally (see [Tests](#tests)).

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`CODING_STANDARDS.md`](CODING_STANDARDS.md). AI agents follow [`AGENTS.md`](AGENTS.md).

## License

Proprietary. All rights reserved (Design D-031).
