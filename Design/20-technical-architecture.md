# 20 — Technical architecture

Status: **Agreed (round 10).** Q79–Q83 were decided as D-085 to D-089. Built on D-004, D-010, D-011, D-026, D-029, D-030, D-031, D-047 to D-050, D-075, D-077 and D-084, and docs 11, 13 and 16. D-109 later amended §6 and §7. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Scope

This document defines:

- the processes, crates and threads;
- the network protocol and how replication works;
- the persistence implementation;
- the server lifecycle;
- the client architecture;
- security, observability and testing.

Most of it puts together decisions already taken; the new choices are marked *Proposed*.

## 2. Processes

*Agreed (D-085).*

```text
 ┌──────────── Windows PC (MVP, D-029) ─────────────┐
 │                                                  │
 │  nomad-client  ──QUIC/TLS, 127.0.0.1──►  nomad-server  ──►  SQLite file
 │  (wgpu + winit,                          (headless,          (per-user data
 │   view-model, UI)                         authoritative,      directory)
 │                                           one Universe)
 └──────────────────────────────────────────────────┘
```

- **`nomad-server`** is headless and authoritative. It hosts one Universe (D-021, D-026). In the MVP it runs on the player's PC, on Windows.
- **The client starts the server** in the background if it is not already running. **The server keeps running after the client exits**, so the Universe goes on (D-026).
- If the PC shuts down, the server stops. At its next start it catches up (D-047).
- **"Start the Universe when I log in to Windows"** is opt-in. It registers the server as a per-user logon task; no Windows service and no admin rights are needed.
- **MVP networking is localhost only:** the server binds to 127.0.0.1.
- *Later:* a Linux container image of the same `nomad-server`, with PostgreSQL and real certificates (D-029, D-030). Same binary, different configuration.

## 3. Crates

*Agreed (D-086).*

| Crate | Owns | Depends on |
|---|---|---|
| `nomad-types` *(new)* | Shared primitives: `SimId`, `CommanderId`, `Tick`, `Ticks`, units, size classes, small enums | `serde` |
| `nomad-sim` | `UniverseState`, `step`, rules (docs 10–16), AI behind `CommanderView` (D-077), the single command validator, snapshots | `nomad-types` |
| `nomad-protocol` *(new)* | Wire messages, framing, protocol version | `nomad-types` |
| `nomad-server` *(new)* | Wall-clock pacing, network, sessions, replication, journal and snapshots (SQLite), recovery, catch-up, operator commands | `nomad-sim`, `nomad-protocol` |
| `nomad-client` | Renderer, input, UI, network client, view-model store | `nomad-protocol`, `nomad-types` |
| `nomad-harness` | Headless fast-forward runs, pacing and AI acceptance runs | `nomad-sim` |
| `nomad-link` *(D-125)* | The client side of the connection: connect, pinning, handshake, channels, reconnect; used by the client and the integration tests | `nomad-protocol`, `nomad-types` |

- **The client never depends on `nomad-sim`.** That keeps the client from simulating anything (C-03), and keeps rules code out of the shipped client.
- `nomad-sim` re-exports its primitives from `nomad-types`. The existing types move there unchanged.

## 4. Server threads

*Proposed.*

| Thread | Does |
|---|---|
| **Simulation** (one OS thread, owns `UniverseState`) | *(D-182: it also extracts each connected commander's view through `CommanderView` after the step; the network thread diffs it.)* Paces ticks against a monotonic clock. Each tick it takes the pending inputs, runs `step`, and publishes `TickOutputs` (change sets, visibility sets, command results, announcements). It encodes a snapshot every 5 min at a tick boundary. |
| **Network** (`tokio` runtime) | Runs the `quinn` endpoint, one task per connection, envelope checks (authentication, size and rate limits) and replication sends |
| **Persistence** | Appends to the journal in batches, flushing at least once per second (D-048). Writes snapshots and maintains read models. |

Channels connect the threads. The simulation thread never blocks on I/O. If encoding a snapshot ever costs more than the tick budget, the state is cloned and encoded on the persistence thread instead.

## 5. Protocol

*Agreed (D-087).*

- **Transport:** QUIC via `quinn` over TLS 1.3 with `rustls`. In the MVP the server generates a self-signed certificate on first start, and the client pins its fingerprint.
  > **Amended by D-120 to D-124 (N3 plan):** the server listens on an OS-chosen port and writes port, fingerprint and token to `<universe>/server.json` (D-120; superseded by D-150: a fixed default port with `--bind` and `--port` overrides, `server.json` kept); certificate and token are new at every start (D-121; superseded by D-151: they persist, with `connect-info` and `rotate-token` commands); validator rejection reasons go on the wire with "unknown / not yours / not visible" collapsed into `NotFound` (D-123); a second connection for a commander replaces the first (D-124).
- **Streams:** one reliable bidirectional control stream (session, commands, acknowledgements) and one reliable unidirectional state stream. There are **no unreliable datagrams in the MVP**; head-of-line blocking does not matter on localhost. Revisit for hosting.
- **Encoding:** length-prefixed `postcard` frames of one versioned `enum` per direction. Client and server must have **exactly the same protocol version**, because they ship together in the MVP.
- **Messages:**

  | Direction | Message | Purpose |
  |---|---|---|
  | Client → server | `Hello { protocol, build }` | Version check |
  | Client → server | `Auth { token }` | Authentication |
  | Client → server | `Command { client_seq, command }` | A player command |
  | Client → server | `Ping` | Liveness and round-trip time |
  | Server → client | `Welcome { commander, universe, tick, clock }` | Session start, including the tick-to-UTC mapping |
  | Server → client | `CatchUp { progress }` | Progress while the server catches up (D-047) |
  | Server → client | `CommandResult { client_seq, applied_tick \| rejected(reason) }` | The outcome of each command |
  | Server → client | `ViewSnapshot` / `ViewDelta { tick, changes }` | The commander's view: a full copy, then changes |
  | Server → client | `Announcement` / `Notification` | Siege information and the away report |
  | Server → client | `Pong` | Reply to `Ping` |
  | Server → client | `Bye { reason }` | End of session |

- **As built in N1** (implementation details within D-087):
  - **Frozen layout:** `Hello` stays variant 0 with `protocol` as its first field, and `Bye { VersionMismatch }` stays variant 0. A version mismatch can then be reported between builds whose layouts differ. That matters because the server outlives the client (D-026).
  - **Frame limits:** 16 MiB from server to client, 64 KiB from client to server.
  - **IDs:** `UniverseId` is a `u64`, shown as the 8-character base-32 ID (D-107). Commands are grouped by target (`Command::Fleet { fleet, order }`, later `Holding`, economy and doctrine).
  - **Added in N3** (protocol version 2): a nonce on `Ping`/`Pong`.
- **As built in N3** (`plans/N3-transport-and-sessions.md`; D-120 to D-125, D-150, D-151):
  - **Endpoint:** a network thread with a current-thread `tokio` runtime, the `quinn` endpoint, a hub task and one task per connection. A bridge thread moves the simulation's events into the runtime, so the simulation thread never waits on the network. TLS 1.3 with the `ring` provider, ALPN `nomad`, no 0-RTT.
  - **Address:** UDP `127.0.0.1:31790` by default; `--bind`/`NOMAD_BIND` and `--port`/`NOMAD_PORT` override it (D-150). 31790 sits below every operating system's ephemeral range. The container image binds 0.0.0.0 and is published on the host's loopback only.
  - **Identity:** certificate, key and token in `<universe>/identity.json`, made at `create` and kept (D-151); `rotate-token` replaces them while the server is stopped. `server.json` gives a local client the address, fingerprint and token, and `connect-info` prints them as a `nomad://` import string for a client that cannot read the server's files.
  - **Sessions:** `Hello` within 5 s (version read from the frozen prefix), `Auth` within 5 s (constant-time comparison), `Welcome`; commands checked for a strictly increasing `client_seq` and a rate limit (20/s, bursts of 60); at most 4 connections; 30 s idle timeout with 5 s keep-alive. A malformed or out-of-order message ends the session with `Bye { ProtocolViolation }`.
  - **Results:** the hub numbers each commander's simulation inputs itself and answers each command from the tick's outcomes, with validator reasons mapped to the wire enum (D-123). Commands during catch-up or after the season end are refused (`CatchingUp`, `SeasonOver`).
  - **Online state:** a commander counts as connected while it has a session. The hub submits the change only while live; the simulation thread disconnects everyone when catch-up starts (D-047) and, at a start without catch-up, the commanders restored as connected.
  - **Client:** the `nomad-link` crate (D-125), with `Connection` for async use and `Link` on a background thread with `std` channels and reconnects. The state stream is opened and stays empty until N4.
- **Commands:**
  - The server checks only the envelope: size, rate and session.
  - The simulation's single validator (D-077) checks the meaning: ownership, resources, rules.
  - That is the same path the Deputy, the Strategist and the raider director use.
  - A command received during tick *t* applies at *t + 1* (doc 11 §6).
- **Replication** (doc 16 §6, D-075):
  - The server tracks, per client, which entities that client already knows about, and sends creates, updates and removes.
  - Content: public data, the commander's own assets, sensed entities, last-known changes, and announcements addressed to that commander.
  - Rates: 10 Hz for the ops area being viewed, 2 Hz for other systems where the commander has a presence, every 5 s for the galaxy summary.
  - **Deadlines are sent instead of countdowns.**
- **Time on the client:** the client shows Universe time from the tick-to-UTC mapping it receives. It renders entities about 100 ms behind the latest tick, interpolating between updates.

## 6. Persistence

*Agreed (D-088).*

- **Storage:** `rusqlite` with bundled SQLite, in write-ahead-log mode. One file per Universe in the per-user data directory (`%LOCALAPPDATA%\Nomad\universes\<id>\universe.db` on Windows). The path comes from the `directories` crate.
- **Tables:**
  - `meta`: format, build, seed, season.
  - `snapshots`: tick, build, format, data.
  - `journal`: tick, sequence, kind, payload, build. Kinds include state-hash checkpoints (D-109).
  - read-model tables, for example standings.
- **Snapshots are not compressed in the MVP.** They are a few MB at stage-2 scale. Compression can be added if size becomes a problem.
- **Retention follows doc 11 §8.** Upgrades always go through a clean-shutdown snapshot (D-048).
- *Later:* PostgreSQL, with the same tables and model (D-030).
- **As built in N2** (implementation details within D-088, D-107, D-109, D-110 and D-116):
  - Journal rows have the kinds `operator`, `commander` and `state_hash`. A checkpoint is the last row of its tick, with the `postcard`-encoded state hash as its payload. It is written every 600 ticks (`ServerConfig::checkpoint_interval`), during catch-up as well. The snapshot of the same tick reuses the hash. Replay compares each checkpoint and reports the outcome in the start report (checkpoints checked and mismatches). It logs every mismatch as an error and carries on (D-109). A journal without checkpoints replays as before. At stage 1 a checkpoint costs about 0.2 ms in a Debug build and 15 µs in Release.
  - The journal floor is the newest kept snapshot at or before 48 h before the latest snapshot (D-110). The journal therefore covers 48–49 h, and recovery can fall back to any snapshot in that window. `status` shows the checkpoint count and the newest checkpoint.
  - Every commander is offline during catch-up (doc 11 §7, D-047). Some can still count as connected when a catch-up starts: after a sleep (D-117), or from the snapshot and replayed journal of a restart. The first catch-up tick therefore carries a journaled `Disconnected` input for each of them. It uses `client_seq` `u64::MAX`, so it applies after the commander's own inputs of that tick. Replay stays identical, and the simulation needs no rule change.
  - At the season's final tick (`season_end`, 8–12 weeks from tick 0) the simulation thread stops stepping. It writes a snapshot and idles in the `SeasonOver` phase until shutdown. Inputs are refused with `SubmitError::SeasonOver`, and catch-up never goes past the final tick. `status` reports an ended season (D-116).
  - `meta` also holds the layout format, the Universe's display name, the creation parameters, a rules hash (filled once the server records the season's rules) and the journal floor; `snapshots` also holds each snapshot's state hash. `synchronous = FULL`, so a committed journal batch survives a power cut.
  - The lock is `universe.lock` beside the database, an OS file lock. Acquiring it retries for up to 0.5 s before refusing, because a lock can stay held for a moment after its owner let go (a child process forked meanwhile holds a copy on Linux; Windows can release it just after the process exits).
  - The Universe ID's 8 characters use Crockford's base-32 alphabet in lower case (no i, l, o or u), so IDs read back without confusion and directory names match on case-insensitive file systems.
  - On Linux the data directory is `~/.local/share/Nomad` (what `directories` returns for the app name); Linux hosting is outside the MVP.

## 7. Server lifecycle

*Derived* from doc 11 §7 and D-047.

1. **Start:** take a lock on the Universe file (a second server on the same Universe is refused), load the latest snapshot, replay the journal while checking state hashes (D-109), then catch up to the wall clock.
2. **During catch-up**, clients may connect, but they receive only `CatchUp { progress }`, and their commands are rejected until the server is live.
3. **Live** play. If the server falls more than 1 minute behind UTC, as after the PC slept, it catches up again as in step 2 (D-117).
4. **Clean shutdown**, triggered by the client's "Shut down Universe" or an operator command, writes a final snapshot and exits.
5. **Crash:** the next start recovers from the snapshot and journal (at most about 1 s of commands lost, D-048).
6. **Season end:** after the season's final tick the Universe stops advancing. The server serves the final state read-only and refuses commands until it is shut down (D-116).

## 8. Client architecture

*Proposed.*

- **Main thread:** the `winit` event loop, input, UI and `wgpu` rendering. These are the phase-1 M1 structures.
- **Network:** a background thread running a small `tokio` runtime with the `quinn` client. It hands messages to the main thread over channels.
- **View-model store:** built from `ViewSnapshot` and `ViewDelta`. Render and UI code read **only** the view-model (phase-1 ground rule 3). The phase-1 fixture schema grows into a saved copy of this view-model (D-028), so the fixtures stay useful for tests and golden images.
- **Without a server**, the client shows a start screen: create a Universe, continue one, or show catch-up progress (doc 17).

## 9. Security and robustness

*Derived* from `CODING_STANDARDS.md` §5. The details are proposed.

- **Bound everything that arrives:** malformed frames, oversized messages and unknown versions are rejected and logged, never turned into a panic. Each message type has a maximum size; each session has a command rate limit.
- **MVP authentication:** on first start the server creates the local commander and a random token, stored in the per-user data directory with user-only permissions. The client reads the token from there. *Amended by D-151 and D-122:* the token is created once, kept in the Universe's data directory and published in `server.json`, protected by mode 0600 on Linux and the profile's per-user ACL on Windows; `connect-info` prints it for a client that cannot read those files, and `rotate-token` replaces it. Accounts and external identity are hosting work, outside the MVP.
- Clients never receive entities they have not detected (doc 16 §6), so a modified client learns nothing extra.

## 10. Observability

*Proposed.*

- Everything logs through `tracing`.
- The server logs tick-time statistics (mean and p99 every minute), catch-up speed, connected clients, journal lag and snapshot duration.
- The headless harness reports the same numbers.
- OpenTelemetry export is hosting work.

## 11. Testing

*Agreed (D-089).*

| Level | What | Where |
|---|---|---|
| Unit | Per crate, including `nomad-protocol` round-trips and rejection of malformed frames | CI (both OS, Debug) |
| Determinism | Same seed gives the same hash; snapshot and restore change nothing (D-049) | CI |
| **Client–server integration** | Start `nomad-server` on localhost, connect a headless test client, run commands, check acknowledgements, replication and catch-up after a restart | **CI (both OS, Debug)** |
| GPU | Golden images, offscreen render tests | `#[ignore = "needs a GPU adapter"]`, run locally |
| AI acceptance and pacing | D-083, D-058, D-070 over simulated days | `#[ignore]`, long runs in the harness, run manually; no scheduled CI job in the MVP |

## 12. New dependencies

All of these have permissive licences (D-031):

| Crate | Licence | Purpose |
|---|---|---|
| `quinn` | MIT / Apache-2.0 | QUIC |
| `rustls` | Apache-2.0 / ISC / MIT | TLS |
| `rcgen` | MIT / Apache-2.0 | Self-signed certificate |
| `tokio` | MIT | Async runtime |
| `rusqlite` with bundled SQLite | MIT; SQLite is public domain | Persistence |
| `directories` | MIT / Apache-2.0 | Per-user data directory |
| `glyphon` | MIT / Apache-2.0 / Zlib | HUD text, for phase-1 M6 |

The bundled SQLite build needs a C compiler. Both CI runners have one.

## 13. Stress test

1. **Two processes for a single-player MVP** cost more than one. D-026 requires a Universe that outlives the client, though, and having the client start the server hides the complexity from the player.
2. **Reliable streams only** is fine on localhost. A hosted server will want datagrams for high-rate updates. Revisit at hosting time, not before.
3. **Requiring an exact protocol version match** means client and server must always be the same build. That is natural while they ship together; a compatibility policy is hosting work.
4. **Encoding a snapshot on the simulation thread** could cause a hitch at stage-2 scale. The clone-and-encode fallback (§4) is ready if measurements say so.
5. **Interest management** can grow complex. Start simple (§5): full detail for the viewed ops area, summaries elsewhere. Optimise only from measurements.
6. **The SQLite C build** adds a toolchain dependency. Both CI runners provide it, and a pure-Rust alternative can be revisited if it ever causes trouble.
