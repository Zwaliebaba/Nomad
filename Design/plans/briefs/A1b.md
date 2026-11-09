You are building work package **A1b** of the Nomad repository: the second half of the Deputy. You work in an isolated git worktree based on the head of `claude/bold-galileo-6lqhoh` (S2–S8 and A1a are built there). I (the main session) review your commits, cherry-pick them onto the branch and push. **Do not push, do not open PRs, do not touch other branches.** Two other agents work in parallel: one on A2 (the Strategist's economy, also in `nomad-ai` and phase 3), one on the D-279 tuning (economy values, D-281's per-resource yield, harness pace measures). Keep to the Deputy's files where you can, so the merges stay small.

## Read first

- `AGENTS.md`, `CODING_STANDARDS.md`.
- `Design/plans/A1-deputy.md` in full, especially §5.3–§5.6, §5.8 (evacuation), §8 tasks 5–9, §9, and §12 (as built in A1a; its "Left for A1b" list is part of your scope).
- Decisions D-199 to D-211, D-016, D-052, D-079, D-082, D-114, D-144, D-145, D-147, D-148, RK-10 (doc 04), and the new D-285 to D-288 in `Design/03-decision-log.md`.
- A1a's code: `crates/nomad-ai/` (start at `deputy/` and its `fake.rs`), `crates/nomad-sim/src/ai/`, `crates/nomad-sim/src/doctrine/`, and `CommanderView` in `crates/nomad-sim/src/sensors/`.

## Scope (A1 plan §8 tasks 5–9, plus A1a's hand-over)

1. **Threat and alert** (§5.3) through `View` and the inbox. Extend `View` with sensed hostiles (wrapping `CommanderView::sensed_fleets_in`), sector strengths and abilities (wrapping `DefenceView`), and fleet hull. `AlertStarted` and `AlertEnded` go to `TickOutputs::deputy_events`.
2. **Alert actions** (§5.4): the alert power profile and its restore, garrison launch per fleet, SHLD SURGE and FLAK SCREEN on their preset triggers; **convoy evacuation** (§5.8, D-210), taking precedence over the mechanical hold in `convoy.rs`'s `Watched` state machine.
3. **Engagement and roles** (§5.5, §5.6, D-207): target priority with explicit `Attack` only when the choice changes, leash from the role's anchor, retreat by fleet hull, the four roles with their idle and alert behaviour, repair at dock, default-role resolution for fleets without an override (rebuild memory's `successors` helps), and a lapsed or released fleet returning to its role.
4. **D-286:** the start kit begins as two fleets, a 3-VX garrison and a 2-MULE reserve (an S4 amendment; re-record the pins).
5. **D-285:** the contested-gate check counts only hostiles in the convoy owner's visibility set (`economy/logistics.rs`); a test showing an undetected armed fleet at a gate does not hold the convoy.
6. **Attacker fleets in combat events,** so the inbox names the attacking fleet when it is sensed (§4.4, A1a gap 5).
7. **Harness `--deputy`** (§9's six scenarios) and the **budget measurement** (D-203): Deputy cost at stage 1 in a release build, mean and p99, and catch-up speed with Deputies (≥ 500×). S5b's breach scenario re-measured with the Deputy's default ability use (D-147). A release build is allowed for this measurement only; delete `target/release` afterwards.
8. **Docs:** doc 12 §5 gains RK-10's addendum and notes for D-207 to D-211; doc 13 §4 becomes agreed text (D-206); doc 30's A1 row; the A1 plan's status and §12.

Out of scope: the Strategist and A2's plans, raider logic (A3), any economy value tuning.

## Rules

- **Do not invent game rules.** Everything must trace to the plan or a decision. Where the plan leaves a gap, choose the narrowest reading, label it *Proposed* in a doc comment, and list it in your report. If a gap blocks you, stop and report rather than guessing. Do not edit `Design/03-decision-log.md` or `Design/02-open-questions.md`; I number and record decisions (next free D-291, Q268).
- Cite design IDs in doc comments and commit bodies.
- Determinism: no clock, I/O, threads, `HashMap` iteration; sorted iteration; `libm`; integer per-mille scores with `SimId` tie-breaks.
- No `unwrap()` outside tests; `expect("invariant: ...")` only for real invariants. Suppressions only as `#[expect(lint, reason = "...")]` on the smallest scope. Never weaken lints or skip tests.
- Tests: fast unit tests run in CI. Anything that takes more than a few seconds in Debug or opens a socket gets `#[ignore = "<what>; CI runs fast tests only (D-190), run with --ignored"]` and must still pass with `--ignored`. Plan §9's competence property test and the snapshot-during-latency determinism test belong in A1a; keep them fast or mark them per D-190.
- Commits: logical and each one must compile and pass tests on its own (I cherry-pick them). Imperative subject ≤ 72 characters, body explains why and cites IDs. End every commit message with:

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01Krm4nzyMHAMpjsFQR37xEP

  No model names anywhere else in commits or code.

## Disk and builds

Disk is the binding constraint in this container. Use the worktree's own `target/` (the default). `.cargo/config.toml` keeps debug info split on Linux; leave it. Build `--release` only for the budget and pacing measurements, and delete `target/release` right after. Do not run `cargo test --workspace -- --ignored` while another workspace build of yours is running. If `df -h /` shows less than 4 GB available, stop building and tell me. I run the Windows-target clippy myself before pushing.

## Before reporting

Run in the worktree, all green:

    cargo fmt --all
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo build --workspace --locked
    cargo test --workspace --locked
    cargo test -p nomad-sim -p nomad-ai --locked -- --ignored   # the harness --ignored run takes about 2 h in Debug; run its tests for your own modes only, by name

Then report: the commit list (hash and subject), the check results, the snapshot `FORMAT`, rules and protocol versions you ended on, every *Proposed* reading you made (with file and line), any conflict or gap you found between the plan, the decisions and the built code, and what the next packages (A2, A3, A4) will need from your seams.
