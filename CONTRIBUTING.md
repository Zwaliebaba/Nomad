# Contributing

## Design first

Gameplay, simulation and architecture changes start in `Design/`, not in code. A change that alters behaviour needs three things:

1. An agreed design document or decision to point to.
2. If none exists, a proposal first: a design document update plus an entry in `Design/03-decision-log.md`, or a question in `Design/02-open-questions.md`.
3. A PR description that cites the relevant IDs (`R-nn`, `D-nnn`, `Qnn`).

Refactors, tooling and fixes that do not change specified behaviour can go straight to a PR.

## Workflow

1. Branch from `main`. Use a short descriptive name such as `feature/ops-grid-pass` or `fix/grid-fade`.
2. Keep each PR to one topic. Fill in the pull request template.
3. Run the local checks below.
4. CI must be green before merge. Do not disable lints, tests or CI steps to get there.
5. At least one review before merging to `main`.

## Local checks

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
```

CI runs only fast tests (D-190). Network tests, long simulations and the GPU tests are `#[ignore]`. Before merging work that touches the server, the network or long-run simulation behaviour, run them all locally:

```bash
cargo test --workspace -- --ignored
```

After changing render code, run at least the GPU tests (they need a GPU adapter or a software Vulkan driver):

```bash
cargo test -p nomad-client -- --ignored
```

In VS Code, the `Local checks`, `GPU tests` and `Ignored tests (network, long simulations, GPU)` tasks run these.

## Commit messages

The subject line is in imperative mood ("Add ops grid pass", not "Added …") and at most 72 characters. The body explains why the change is needed and cites design IDs where relevant.

## CI policy

- Debug profile only (D-010). There are no Release builds in CI.
- Linux and Windows runners (D-011).
- Fast tests only (D-190): the `#[ignore]`d network tests, long simulations and GPU tests run locally, as above.
- `cargo deny` checks licences and advisories against `deny.toml` (D-031).
- Pushes to any branch run CI. Same-repository PRs show those results; PRs from forks run CI themselves.

## Coding standards

See `CODING_STANDARDS.md`. Formatting and lints are enforced by `rustfmt`, `clippy` and CI. Everything else is enforced in review.
