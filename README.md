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
  nomad-client/         Game client (phase 1: the visual slice; not started yet)
  nomad-sim/            Deterministic simulation core (Design doc 11): tick, timers, SimId, random streams, snapshots
  nomad-harness/        Headless fast-forward runner: pacing runs and determinism checks
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

Run the headless simulation harness (fast-forward, reports the state hash and speed):

```bash
cargo run -p nomad-harness -- --ticks 36000 --diagnostics 10000
```

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
