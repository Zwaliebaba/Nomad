# AGENTS.md

Instructions for AI coding agents working in this repository (Claude Code, Codex, Copilot and others). Human contributors: see `CONTRIBUTING.md`. If anything here conflicts with an explicit instruction from the owner in your session, follow the owner and point out the conflict.

## The project

Nomad (working title) is a persistent multiplayer space RTS in Rust:

- Commanders grow an empire from a mother station in one shared **Universe** (up to 100 commanders, seasons of 8–12 weeks) that keeps running while they are offline.
- A **Deputy** AI always executes each commander's standing orders (doctrine). While the player is away it runs the empire alone. Offline protection is clock-based: see `Design/12-offline-play-and-deputy.md`.
- MVP: one human player and up to 9 AI commanders, on a local headless server.
- Client: a custom `wgpu` + `winit` renderer drawing an isometric voxel scene, on Windows. Server: Linux; `hecs`, `tokio` + `quinn`, SQLite (D-029, D-030).
- License: proprietary (D-031). Only permissively licensed dependencies may ship.

## Current phase: build (the MVP design is complete)

- `Design/` is the source of truth. Read `Design/README.md` first, then `Design/00-glossary.md`.
- Implement only what an agreed design document or decision covers. Every MVP document is agreed. Pick work packages from `Design/30-mvp-scope-and-milestones.md` §2, in the order of §4.
- The `nomad-sim` core (doc 11: tick, deadline timers, `SimId`, random streams, snapshots, headless harness) is being built now (D-051). **Game rules** come only from agreed design documents: Universe structure (doc 10), offline rules and the Deputy (doc 12), economy (doc 14), combat and defence (doc 15), information model and fog of war (doc 16), AI commanders, the Deputy and raiders (doc 13). Do not invent rules.
- Do not invent gameplay rules in code. If a task needs a design decision that does not exist yet, stop and say so. Then propose the decision, or add a question to `Design/02-open-questions.md`.
- `Design/03-decision-log.md` holds every decision:
  - **Agreed** decisions are binding. Do not reopen them silently.
  - **Provisional** decisions are assumptions. Flag it when your work depends on one.
  - To change a decision, add a new entry that supersedes the old one. Never rewrite an old entry.
- Cite design IDs (`R-nn`, `D-nnn`, `Qnn`, or a doc and section) in doc comments, commit messages and PR descriptions when code implements them.
- When writing design documents, label each statement as one of these:
  - **Stated:** the owner said it.
  - **Derived:** it follows from stated requirements.
  - **Proposed:** it needs the owner's approval.

## Terminology

- The total game space is **the Universe**: in player-facing text, documents and domain types (`Universe`, `UniverseState`). Never call it "the world" or "the map".
- *World* is fine as a technical term, for example renderer world space or an ECS `World`.
- Use the glossary's vocabulary: holding, commander, Deputy, ops grid, module, elevation tier, ship plane.
- Phase-1 rule: player-facing text uses no aviation terms. Say ELEV, station-keeping, quadrant.

## Commands

Run these before every commit. CI runs the same commands with `--locked`.

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
```

After changing render code, also run the GPU tests, which CI cannot run: `cargo test -p nomad-client -- --ignored`.

## CI rules

- CI builds the **Debug** profile only (D-010). Do not add `--release` builds or a build-configuration matrix.
- CI runs on Linux and Windows (D-011). Code must build on both. Put platform-specific code behind `cfg`.
- Never weaken lints, skip tests or remove CI steps to get a green build. Fix the code instead. A local suppression must use `#[expect(lint, reason = "...")]` on the smallest possible scope.

## Coding rules (summary)

`CODING_STANDARDS.md` is authoritative. The rules most often missed:

- No `unwrap()` outside tests. Use `expect("invariant: ...")` only for real invariants. Malformed input from the network or from files must never panic the server.
- `unsafe` is denied workspace-wide. An exception needs the owner's agreement and a `// SAFETY:` comment.
- Log with `tracing`, not `println!`.
- Render code reads player-facing strings and values from fixtures or the view-model, never from string literals (phase-1 ground rule 3).
- Add new dependencies to `[workspace.dependencies]` in the root `Cargo.toml` and justify them in the PR.

## Repository layout

```
Design/          Design documents; phase1-visual-slice/ holds the owner's phase-1 package and assets
crates/          nomad-client (phase 1), nomad-sim (deterministic core, doc 11), nomad-harness (headless runner),
                 nomad-types (shared primitives, doc 20 §3), nomad-protocol (wire messages and framing, doc 20 §5),
                 nomad-server (server shell: persistence, recovery, catch-up; N2 plan); further crates follow doc 20 §3
tools/preview/   WebGL preview of the phase-1 scene for design review (not the game client)
.github/         CI, Dependabot, PR template
```

## Git

- Commit subject: imperative mood, at most 72 characters. The body explains *why* and cites design IDs.
- Do not commit `target/`, local screenshots or downloaded fonts. The fonts are OFL-licensed and the phase-1 package says they are not bundled. Milestone screenshots under `screenshots/milestones/` are the exception (phase-1 doc 06).
- Do not rewrite history on shared branches. Do not push to `main` directly.

## When unsure

Ask. The owner prefers questions up front over assumptions, and expects proposals to be stress-tested, not just agreed with.
