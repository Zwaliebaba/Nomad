# Nomad

*Working title (Design D-018).*

Nomad is a persistent multiplayer space RTS written in Rust. Each commander starts with a mother station and grows an empire of factories, shipyards and fleets. The Universe they share keeps evolving when they log off. Each commander's **Deputy** always executes their standing orders, and while the player is away it keeps the standard actions running and defends their holdings. The look is an isometric voxel diorama with a fixed-pitch camera, 4-way snap rotation and a military HUD.

Targets: one Universe of up to 100 commanders, played in seasons. The MVP is one human player against up to 9 AI commanders, running on a local headless server.

## Status

The project is in the **design phase**. There is no gameplay code yet. The Cargo workspace contains one placeholder crate, so that tooling, lints and CI are already in place. Start at [`Design/README.md`](Design/README.md).

## Repository layout

```
Design/                 Design documents (source of truth), including the phase-1 visual slice package
crates/
  nomad-client/         Game client (placeholder; phase 1 is the visual slice)
.github/                CI workflow, Dependabot, pull request template
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
