# Coding standards

These rules apply to all Rust code in this repository. Tools enforce formatting and lints, and CI fails on any violation. Review enforces the rest. When a rule here conflicts with an agreed design decision, the decision wins and this file gets fixed.

| Area | Enforced by |
|---|---|
| Formatting | `rustfmt` (`rustfmt.toml`), `.editorconfig`, CI `cargo fmt --check` |
| Lints | `[workspace.lints]` in `Cargo.toml`, `clippy.toml`, CI `clippy -D warnings` |
| Toolchain | `rust-toolchain.toml` (pinned) |
| Everything else | Review |

## 1. Toolchain

- The compiler version is pinned in `rust-toolchain.toml`, edition 2024, resolver 3. `rust-version` in `Cargo.toml` matches the pin.
- Bump the toolchain in its own PR. That PR also fixes any new lints and updates both files.

## 2. Formatting

- `cargo fmt --all` before every commit. Do not align code by hand, and do not use `#[rustfmt::skip]` unless a table of data is clearer unformatted.
- Text files use LF line endings on every OS (`.gitattributes`). The encoding is UTF-8.

## 3. Lints

- Every crate starts with `[lints] workspace = true`. Do not override workspace lint levels per crate.
- The workspace enables `clippy::pedantic` and a few restriction lints (`unwrap_used`, `dbg_macro`, `todo`, `print_stdout`, `print_stderr`, `undocumented_unsafe_blocks`). `unsafe_code` is denied. CI treats warnings as errors.
- When a lint is wrong for a specific case, suppress it on the smallest scope with `#[expect(clippy::lint_name, reason = "why")]`. `#[expect]` fails once the suppression is no longer needed, which `#[allow]` does not. Never suppress a whole lint group at crate level.

## 4. Naming and vocabulary

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/naming.html) for casing and naming.
- Use the vocabulary in `Design/00-glossary.md` for domain types: `Holding`, `Commander`, `Deputy`, `OpsGrid`, `Module`, `ElevationTier`.
- The total game space is the **Universe** (`Universe`, `UniverseState`, `UniverseId`). *World* is fine as a technical term, for example `world_to_view` in the renderer or an ECS `World`.
- Wrap identifiers in newtypes (`CommanderId(u32)`, `HoldingId(u32)`), not bare integers.
- Quantities carry their unit, either as a newtype (`Voxels(f32)`, `Megawatts(u32)`) or as a suffix (`range_km`, `cooldown_s`, `width_px`). A bare `f32` called `range` is a review finding.
- Abbreviations are allowed only where the glossary or the HUD uses them (HUD, ELEV, CMD). Otherwise write words out.

## 5. Error handling

- Library crates define typed errors with `thiserror`. `anyhow` is only for binary entry points and tools.
- No `unwrap()` outside tests (lint-enforced). `expect("invariant: …")` is allowed only for real invariants, and the message names the invariant.
- Nothing that arrives from outside the process may cause a panic: network messages, save files, configuration, assets. Validate the input, reject it and log it.
- Do not use `todo!()` or `unimplemented!()` on code paths that can be reached. Return an error or leave the feature out.

## 6. Logging and diagnostics

- Use `tracing` with structured fields (`info!(holding = %id, "shield sector weakened")`). `println!` and `eprintln!` are lint errors outside tests.
- Log levels: `error` means operator action is needed. `warn` means unexpected but handled. `info` means a lifecycle event. `debug` and `trace` are for development.

## 7. Unsafe code

`unsafe` is denied workspace-wide. If an exception is unavoidable, all of the following apply:

- The owner agrees to it in the PR.
- It is confined to the smallest possible module, with `#[allow(unsafe_code)]` on that module.
- Every `unsafe` block carries a `// SAFETY:` comment that explains why the invariants hold.

## 8. Simulation code

*Agreed in doc 11 (D-049, D-050).*

- Authoritative Universe state lives only in the server-side simulation. Clients send commands and render the state they receive (design C-02, C-03).
- Given the same state, commands and seed, the simulation must produce the same result:
  - Time is the tick counter. Do not read wall-clock time (`Instant::now`, `SystemTime::now`) inside the simulation.
  - Do not iterate a `HashMap` or `HashSet` where the order affects the outcome. Use `BTreeMap`, `IndexMap` or sorted vectors.
  - Randomness comes from a seeded RNG that is passed in explicitly. There is no global or thread-local RNG.
  - Logic runs on a fixed tick, never on frame time.
- Every simulated object has a `SimId(u64)`, assigned in increasing order and never reused. A `hecs` `Entity` is a storage handle only and never leaves the simulation.
- Positions and headings are `f64`. Transcendental functions come from the `libm` crate, never from `std`'s platform maths. No fast-math.
- Resources are integer milli-units, never floats.
- Timers are absolute deadline ticks, fired in `(deadline_tick, SimId)` order.
- `nomad-sim` does no I/O, reads no clock, starts no threads and uses no `tokio`.
- Offline and online commanders are simulated by the same code paths and at the same fidelity (design W-05).

## 9. Client and rendering code

- The renderer is data-driven. Player-facing strings and values come from fixtures, or later from the server view-model, never from literals in render code (phase-1 ground rule 3).
- Layout values are in 1920×1080 logical units, scaled uniformly (phase-1 docs 01 and 04).
- Configuration and assets load through `serde` structs. Unknown `format` versions are rejected. Hot reload is allowed in debug builds only.
- The client never mutates authoritative game state. Input becomes commands.
- Performance budgets in design documents are requirements, not targets; for example, the voxel pass must stay under 2 ms of GPU time at 1080p. Measure before optimising, and avoid per-frame allocations in hot loops.

## 10. Dependencies

- Declare versions once, in `[workspace.dependencies]` in the root `Cargo.toml`. Crates use `name = { workspace = true }`.
- Justify every new dependency in the PR: what it does, why the standard library or an existing dependency is not enough, its maintenance status and its license.
- The project is proprietary (D-031). Dependencies that ship in a binary must use permissive licences: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, or MPL-2.0 (file-level copyleft only). GPL, LGPL and AGPL are not allowed in shipped binaries. Build and dev tools are exempt.
- No git or path dependencies outside this workspace on `main`.
- Commit `Cargo.lock`. CI builds with `--locked`.
- Do not commit third-party assets unless their license allows redistribution. The phase-1 fonts are OFL but deliberately not bundled.

## 11. Testing

- Unit tests live next to the code in `#[cfg(test)] mod tests`. Integration tests go in each crate's `tests/`.
- Tests are deterministic: no sleeps, no network, no dependence on wall-clock time. Seed any randomness.
- Every bug fix comes with a regression test.
- CI runs tests in the Debug profile, so keep the suite fast in debug. Mark slow tests `#[ignore]` with a reason, and do not leave them unrun indefinitely.
- Rendering uses golden-image tests with a perceptual tolerance, as described in phase-1 doc 05.

## 12. Documentation and comments

- Every crate has a `//!` crate-level doc. Public items in library crates have doc comments.
- Comments explain *why*. The code shows *what*.
- When code implements a design rule, cite it: `// Shield sectors by azimuth: Design/phase1-visual-slice/docs/01-visual-spec.md §6, D-003.`

## 13. Platforms

- Code builds and tests on Linux and Windows (D-011). Platform-specific code sits behind `cfg(target_os = …)` with an equivalent for each supported platform.
- Use `std::path` for paths, with no hard-coded separators. Do not assume a case-sensitive file system.
