# Work package N2 — Server shell

Status: **Planned and decided** (Q97–Q100 → D-106, D-107, D-109, D-110). Starts when N1 is merged, in parallel with S3. This plan is also the brief for whoever builds N2.

## 1. Goal

A `nomad-server` binary that:

- creates a Universe;
- runs it in real time;
- saves it continuously (journal plus snapshots);
- recovers after a crash;
- catches up after downtime.

There is no networking yet (N3). It is the authoritative host every later package plugs into.

## 2. Inputs

- **Server lifecycle and simulation:** doc 11 §2 (clocks), §6 (journal), §7 (live, catch-up, fast-forward) and §8 (persistence).
- **Architecture:** doc 20 §2 (processes), §4 (threads), §6 (persistence), §7 (lifecycle) and §10 (observability).
- **Decisions:**
  - D-047: catch-up; no raids during catch-up; operator freeze is hosting-only.
  - D-048: snapshots plus journal; flush ≤ 1 s; upgrades go through a clean-shutdown snapshot.
  - D-049, D-050: determinism and a pure simulation crate.
  - D-088: `rusqlite` with bundled SQLite, write-ahead log, per-user data directory.
  - D-099: no snapshot migration before β.
- **Tooling:** the existing stage-1 generator (S2) and the harness.

## 3. Scope

**In scope:**

- **The `nomad-server` crate:** a library for tests plus a binary.
- **CLI:** `create`, `run` and `status`.
- **Server internals:**
  - Universe directory and lock;
  - SQLite schema;
  - simulation thread with wall-clock pacing;
  - persistence thread;
  - recovery;
  - catch-up;
  - clean shutdown on Ctrl+C;
  - snapshot retention;
  - tick statistics.
- **An in-process `ServerHandle` API** (submit inputs, receive outputs) that N3 will put behind the network.

**Out of scope:**

- QUIC, sessions and authentication (N3);
- replication (N4);
- the client starting the server, and start-with-Windows (N5);
- the operator "freeze" (hosting only, D-047);
- PostgreSQL (hosting).

## 4. Design

### 4.1 Commands

- **`nomad-server create`** generates the stage-1 Universe (S2) from a seed and writes `meta` plus the tick-0 snapshot. Its options cover:
  - the seed (random if omitted);
  - the number of AI commanders (3);
  - the difficulty (Normal);
  - the season length (Q97);
  - the data directory.
- **`nomad-server run --universe <id>`** runs that Universe.
- **`nomad-server status`** prints meta, last tick, snapshots and journal size, without running anything.

### 4.2 Data directory and lock

- One directory per Universe: `<data>/universes/<id>/universe.db`. The base directory comes from `directories`, and `--data-dir` or `NOMAD_DATA` override it.
- **Exclusive lock:** `std::fs::File::lock` on `universe.lock`, stable since Rust 1.89. A second server on the same Universe is refused with a clear message.

### 4.3 Schema

| Table | Columns |
|---|---|
| `meta` | Format, build, seed, season start (UTC ms), season length, creation parameters, rules hash |
| `snapshots` | tick, build, format, bytes |
| `journal` | tick, seq, kind, payload (`postcard`), build |

Write-ahead-log mode. The journal is written in one transaction per batch.

### 4.4 Threads

*Derived* from doc 20 §4.

- **Simulation thread:**
  - Owns `UniverseState`.
  - Paces one tick per 100 ms against a monotonic clock. Overruns run back-to-back, and a warning is logged when it falls more than 10 ticks behind.
  - Takes the inputs submitted for the tick and runs `step`.
  - Sends to the persistence thread: the inputs for the journal, every 600 ticks a `StateHash` journal entry after that tick's inputs (D-109), and every 3,000 ticks (and on shutdown) the encoded snapshot.
  - Publishes `TickOutputs` to subscribers.
- **Persistence thread:**
  - Commits journal batches at least once per second.
  - Writes snapshots.
  - Applies retention as in doc 11 §8: the last 2 snapshots, hourly snapshots for 48 h, daily for the season; journal entries older than 48 h are removed (D-110).
- **Clock injection:** a `Clock` trait gives the wall clock in production and a fake clock in tests. That makes pacing and catch-up testable without waiting.

### 4.5 Recovery and catch-up

*Derived* from doc 11 §7–8.

1. **Load and replay:** open the database, load the latest snapshot, and replay the journal entries after its tick. Entries from a **different build** abort startup with a clear error (D-048). At each `StateHash` entry, replay compares the hash. A mismatch is logged as an error with the tick and both hashes, and recovery continues (D-109).
2. **Catch-up:**
   - The target tick is the current UTC time mapped through the `SeasonClock`.
   - The server fast-forwards to it with empty inputs.
   - Catch-up is **marked in the journal** with `CatchUpStart` and `CatchUpEnd` entries, and the simulation sees the mode. That keeps replay identical and later lets S6 and A3 suppress raids (D-047).
   - It logs progress (ticks per second and how far is left), for the client's catch-up screen later (N3/U6).
3. **Live play.**
4. **Ctrl+C**, or `ServerHandle::shutdown`, triggers a clean shutdown: final snapshot, flush, exit.

### 4.6 Build ID

A `build.rs` embeds the git commit, with "unknown" as the fallback. It is written with every snapshot and journal entry.

## 5. Decisions (formerly open questions)

*Agreed: D-106 (Q97), D-107 (Q98), D-109 (Q99), D-110 (Q100).*

- **Q97 — Default season length** for new Universes: **8 weeks**, matching the MVP acceptance run (D-097). `create` can set anything from 8 to 12 weeks (D-020).
- **Q98 — Universe ID:** a random 8-character base-32 ID plus an optional display name.
- **Q99 — State-hash checkpoints:** every 600 ticks in the journal, checked on replay.
- **Q100 — Journal retention:** 48 h; the value is provisional until the volume is measured.

## 6. Tasks, in order

1. Crate skeleton, CLI, `build.rs`, data directory and lock.
2. SQLite schema and the persistence thread: journal batches, snapshots, retention.
3. `create`: generate the stage-1 Universe and write meta plus the tick-0 snapshot.
4. Simulation thread with the `Clock` trait and pacing; `ServerHandle` (submit inputs, subscribe to outputs, shutdown).
5. Recovery: load, replay, check state hashes, refuse other builds.
6. Catch-up, with journaled mode markers and progress reporting.
7. Clean shutdown on Ctrl+C, on Windows and Linux.
8. `status`; tick statistics in the log (mean and p99 per minute).

## 7. Done when

- **CI** is green on both operating systems, with `--locked`.
- **Crash recovery:** an integration test runs a Universe with scripted inputs, "crashes" by dropping the server without a clean shutdown, restarts, and ends at the same state hash as an uninterrupted reference run. At most the last journal batch (≤ 1 s) may be lost, and the test accounts for that by flushing first.
- **Divergence check:** with a wrong `StateHash` entry planted in the journal, recovery logs the mismatch and still completes.
- **Catch-up:** with a fake clock, a Universe whose season started 6 h ago, or that was stopped 6 h ago, ends exactly at the target tick, and its hash equals a continuous run of the same length.
- **Lock:** a second `run` on the same Universe is refused.
- **Retention:** after simulated days, only the snapshots described in doc 11 §8 remain, and journal entries older than 48 h are gone.
- **No panics:** a corrupt database, a missing snapshot or an unknown format each give a clear error.
- **Speed:** catch-up speed for stage 1 is reported. The doc 11 target is ≥ 500×; with no game rules yet, it should be far above that.
