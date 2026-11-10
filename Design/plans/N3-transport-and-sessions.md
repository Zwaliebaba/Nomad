# Work package N3 — Transport and sessions

Status: **Built.** Planned and decided as Q109–Q114 → D-120 to D-125. **Amended by D-150 and D-151** (container trial): a fixed default port with `--bind`/`--port` overrides instead of an OS-chosen port, and a persistent certificate and token with `connect-info` and `rotate-token` instead of new ones at every start; the build follows them. D-152 (external servers) was superseded by D-245 before it was built: the server always runs in Docker Desktop and owns the start screen (N5). **Transport replaced (D-241, D-242):** one WebSocket per session on TCP 31790 replaces the QUIC endpoint, which retires with `quinn`, `rustls`, `rcgen`, the certificate and the `nomad://` import string once the WebSocket endpoint passes the integration test (N6, doc 21 §5). This plan's session logic (hub, limits, token, lifecycle) stays; sessions without an `Origin` keep the token (D-244). This plan is also the brief for whoever builds N3. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Goal

A client on the same PC can connect to the running `nomad-server` over QUIC on 127.0.0.1, prove it is the local player, and play by command:

- it learns the Universe, its commander and Universe time;
- it sees catch-up progress;
- it sends commands and gets each one's outcome: the tick it applies on, or why it was refused.

A headless test client drives all of it in CI on both operating systems (D-089). What the commander *sees* (the view snapshot and deltas) is N4.

> *Note (2026-11-07):* D-190 later took network tests out of CI. The integration tests of §7 are `#[ignore]`d and run locally with `cargo test --workspace -- --ignored`, on Linux and on Windows. Since N3 the protocol has moved on to version 4 (S5b and S6a; doc 20 §5).

## 2. Inputs

- **Architecture:** doc 20 §2 (processes, localhost only), §4 (threads), §5 (protocol, including "As built in N1"), §7 (lifecycle), §8 (client architecture), §9 (security), §10 (observability), §11 (testing).
- **Decisions:**
  - D-085: separate headless server; localhost only in the MVP.
  - D-087: QUIC via `quinn`, TLS 1.3 with `rustls`, self-signed certificate pinned by the client, reliable streams only, length-prefixed `postcard` frames, exact protocol-version match, commands answered with the applied tick or a rejection.
  - D-089: a localhost client–server integration test in CI, Debug, both operating systems.
  - D-107: the Universe ID names the Universe directory.
  - D-116: after the season's final tick the server is read-only and refuses commands.
  - D-117: a live server more than 1 minute behind UTC catches up again.
  - D-077: the simulation's single validator judges a command's meaning; the server checks only the envelope.
- **Simulation rules:** doc 11 §6 (a command received during tick *t* applies on *t + 1*; connect and disconnect are journaled inputs that drive the 120 s grace, sieges and activity windows), §7 (nobody counts as online during catch-up, D-047).
- **Replication boundary:** doc 16 §6 and D-075 belong to N4.
- **Code:**
  - `crates/nomad-protocol`: the two message enums, framing, frame limits (16 MiB down, 64 KiB up), the frozen `Hello`/`Bye` layout, `PROTOCOL_VERSION = 1`.
  - `crates/nomad-server`: `ServerHandle` (`submit`, `subscribe`, `status`, shutdown), `Input`, `SubmitError`, the `Phase` lifecycle, the Universe lock and the data directory.
  - `crates/nomad-sim`: `TickOutputs::commands` carries `(commander, client_seq, outcome)` for every command; `CommandRejection` is the validator's reason.
- **Work in flight:** D-109 (journal checkpoints), D-110 (48 h journal) and D-116 (season-end freeze, with a new `SubmitError` case for "season over") are being added to `nomad-server`. This plan relies on what they do, not on their names.

## 3. Scope

**In scope:**

- **Server endpoint:** a `quinn` endpoint on 127.0.0.1, run by `nomad-server run`, beside the simulation and persistence threads.
- **Identity and discovery:** the self-signed certificate, the token and the connection file (Q109–Q111).
- **Sessions:** the handshake, authentication, Welcome, catch-up progress, commands and results, Ping/Pong, Bye, limits and timeouts, and the connect and disconnect inputs to the simulation.
- **Protocol changes, as one version step:** the Ping nonce (an N1 follow-up), the validator's reason in rejections (Q112), the "season over" rejection and new Bye reasons. `PROTOCOL_VERSION` goes to 2.
- **Client link:** a small library that connects, does the handshake and exchanges messages on a background thread over channels (Q114). C7 builds on it.
- **Integration tests:** in-process and with the real binary, on Linux and Windows.
- **`status`:** shows whether the server is running and on which port.

**Out of scope:**

- replication, the view snapshot and deltas, interest management, announcements and notifications (N4); N3 opens the state stream, and N4 fills it;
- the client starting the server, start with Windows, and "Shut down Universe" from the client (N5);
- the client's view-model store, start screen and catch-up screen (C7, U6);
- hosting: real certificates, accounts, remote connections, datagrams (doc 20 §13 items 2–3).

## 4. Design

### 4.1 Threads

*Derived* from doc 20 §4.

- **Network thread:** one OS thread running a current-thread `tokio` runtime with the `quinn` endpoint, one task per connection. One local player does not need a multi-threaded runtime (*Proposed*).
- **To the simulation:** sessions call `ServerHandle::submit`, a non-blocking channel send, so the network never waits on a tick.
- **From the simulation:** a small bridge thread reads `ServerHandle::subscribe` (a blocking `std` receiver) and forwards events into a `tokio` channel. The simulation never blocks on the network: its subscriber channels already drop events for a subscriber that falls 4,096 events behind (N2).
- **Lag:** if the bridge ever falls that far behind, a session can miss command results. The bridge detects a gap in tick numbers and ends affected sessions with `Bye { Resync }`; the client reconnects. That is a safety net, not a normal path.
- **Shutdown:** the network thread stops when the server shuts down. Every session gets `Bye { ShuttingDown }` first, then the final snapshot is written (doc 20 §7).

### 4.2 Endpoint, certificate and discovery

*Derived* from D-085, D-087 and doc 20 §9; the lifetimes and the file are *Proposed* (Q109–Q111).

- **Bind:** UDP on 127.0.0.1 only, never 0.0.0.0. Nothing off the machine can reach the server. On Windows, a loopback-only bind also avoids the firewall prompt.
- **Certificate:** `rcgen` makes a self-signed certificate for `localhost` and `127.0.0.1` (Q110 decides how long it lives). Its SHA-256 fingerprint goes into the connection file.
- **TLS:** `rustls` with the `ring` provider; TLS 1.3 only; ALPN `nomad`, without a version, because version checks belong to `Hello` (D-087). 0-RTT is off, because it allows replay.
- **Pinning:** the client accepts exactly the fingerprint from the connection file. It uses a custom certificate verifier that compares the end-entity certificate's SHA-256 and still checks the handshake signature. There is no CA and no system trust store.
- **Connection file** (Q109): `<data>/universes/<id>/server.json`, holding the port, the certificate fingerprint, the token, the server's build and protocol version, and its process ID.
  - The server writes it atomically after binding. It removes it on clean shutdown.
  - A file left by a crash is recognised as stale because the Universe lock is free (`UniverseLock::is_held`).
  - The file is readable by the user only (Q111).
- **Token:** 32 random bytes, hex-encoded, compared in constant time (doc 20 §9). It proves that the client runs as the same OS user, who can read the connection file.

### 4.3 Streams

*Derived* from doc 20 §5.

- **Control stream** (client-opened, bidirectional): `Hello`, `Auth`, `Welcome`, `CatchUp`, `Command`, `CommandResult`, `Ping`, `Pong` and `Bye`.
- **State stream** (server-opened, unidirectional): `ViewSnapshot`, `ViewDelta`, `Announcement` and `Notification`. N3 opens it after `Welcome` and sends nothing on it yet; N4 fills it.
- **Framing:** both streams carry the N1 frames (`u32` length plus `postcard` body), read with `FrameDecoder` under the per-direction size limits.

### 4.4 Session lifecycle

*Derived* from doc 20 §5, §7, §9 and doc 11 §6; the timings are *Proposed* (§4.6).

1. **Hello.** The first frame must be `Hello` and arrive within 5 s. The server reads the version with `hello_protocol`, which works for any version thanks to the frozen layout. On a mismatch it sends `Bye { VersionMismatch { server } }` and closes.
2. **Auth.** Next comes `Auth { token }`, within 5 s. A wrong token gets `Bye { AuthenticationFailed }`, and the attempt is logged without the token (`AuthToken`'s `Debug` already hides it).
3. **Second session** (Q113). If the commander already has a session, the server applies the Q113 rule.
4. **Welcome.** The server sends the human commander (`HUMAN_COMMANDER` from `create`), the Universe ID, the last tick and the `SeasonClock`, so the client shows Universe time (doc 11 §2).
5. **While catching up** (start-up or D-117), the session gets `CatchUp { reached, target }` about once a second. Commands are rejected with `CatchingUp` (doc 20 §7). When the server goes live, the session sends nothing special; commands simply start to apply.
6. **Commands.** `Command { client_seq, command }`:
   - Envelope checks: authenticated, within the rate limit, server live, season not over (D-116). Each failure is answered at once with `CommandResult { Rejected { reason } }`.
   - Otherwise the command becomes `Input::Commander(CommanderInput { commander, client_seq, kind: Command(..) })`.
   - The session answers when the tick's outputs list that `(commander, client_seq)`: `Applied { tick }` or `Rejected { Invalid { reason } }` (Q112). The applied tick is the output's tick, so doc 11 §6's *t + 1* is visible to the client.
   - **`client_seq` must strictly increase within a session.** A repeat or a step back is a protocol violation. The simulation orders a commander's inputs by `(commander, client_seq)` (S3); unique numbers keep results unambiguous.
7. **Ping** `{ nonce }` is answered with `Pong { nonce }` at once, outside the tick (N1 follow-up). The client measures the round trip.
8. **End.** `Bye { reason }` from the server, a client close, or the QUIC idle timeout ends the session.
9. **Season over** (D-116). The session stays open, read-only: `Welcome` and, with N4, the view still flow, and commands get `Rejected { SeasonOver }`.

### 4.5 Connect and disconnect inputs

*Derived* from doc 11 §6–7 and D-047.

- **The server owns these inputs.** A session submits `Connected` for its commander once authentication succeeds, and `Disconnected` when it ends, for any reason.
- **If the server is not live,** the network layer keeps only the latest pending state per commander and submits it on the first live tick. A connect during start-up catch-up is therefore journaled when live play begins, and a connect and disconnect both within catch-up leave no trace. That matches doc 11 §7: nobody counts as online during catch-up.
- **D-117 re-catch-up while a session is open.** The simulation thread, when it writes `CatchUpStart`, also writes `Disconnected` in the same tick for every commander in `UniverseState::connected()`. When catch-up ends, the network layer submits `Connected` again for sessions that are still open. Both inputs are journaled, so replay agrees.
  - This touches the D-117 code in `nomad-server` and must land with it or before N3's tests.
- **Disconnect detection** uses the QUIC idle timeout (§4.6). That sits well inside the 120 s grace period of doc 12 §5.3.

### 4.6 Limits and timeouts

*Proposed*; constants in one place, tuned like other numbers.

| Limit | Value | When exceeded |
|---|---|---|
| Hello, then Auth | 5 s each | `Bye { HandshakeTimeout }` |
| Command rate | 20 per second sustained, bursts of 60 (token bucket) | `Rejected { RateLimited }`; the session stays |
| Frame size | N1's limits: 64 KiB up, 16 MiB down | `Bye { ProtocolViolation }` |
| Malformed frame, out-of-order message, `client_seq` not increasing | — | `Bye { ProtocolViolation }` |
| QUIC idle timeout | 30 s, keep-alive every 5 s | session ends; `Disconnected` |
| Concurrent connections | 4, counting unauthenticated ones | refused at the QUIC layer |

The connection cap protects only against a local process flooding the port. Off-machine traffic cannot reach a loopback bind.

### 4.7 Protocol changes (version 2)

*Derived:* the N1 rule says any change to a message type increments `PROTOCOL_VERSION`. All of N3's changes land as one step from 1 to 2:

- `Ping { nonce: u64 }` and `Pong { nonce: u64 }`.
- `RejectReason::Invalid { reason }`, with a wire enum of validator reasons (Q112), and `RejectReason::SeasonOver` (D-116).
- `ByeReason` gains `HandshakeTimeout`, `Resync` and, depending on Q113, `Replaced`.
- **The frozen parts stay frozen:** `Hello` is variant 0 with `protocol` first, and `Bye` is variant 0 with `VersionMismatch { server }` as `ByeReason` variant 0. A version-1 client meeting a version-2 server therefore still gets a readable mismatch.

### 4.8 Client link

*Proposed (Q114).*

- **API:** `Link::connect(universe_dir)` reads `server.json`, opens the QUIC connection with the pinned fingerprint, sends `Hello` and `Auth`, and returns once `Welcome` (or a `Bye`) arrives.
- **Thread model:** the link runs its own thread with a current-thread `tokio` runtime. The caller gets two `std` channels: messages out (`ClientMessage`) and events in (`ServerMessage` plus link events such as `Disconnected { reason }`). That is the doc 20 §8 shape: the client's main thread never blocks on the network.
- **Reconnects:** the link reconnects with backoff (0.5 s, doubling, up to 5 s) and re-reads `server.json` each time, because the port, fingerprint and token change when the server restarts (Q110). It reports each reconnect, so the client knows its `client_seq` and pending results were reset.
- **Boundary with C7:** C7 owns the view-model store, the catch-up screen and command bookkeeping in the UI. The link only moves messages.

### 4.9 `status` and logs

*Derived* from doc 20 §10.

- **`status`:** reports "running on 127.0.0.1:<port>" when the lock is held and `server.json` exists, and "not running" otherwise.
- **Logs:**
  - one line per session start and end (commander, reason, duration);
  - a per-minute line with the session count, commands per second and rejections by reason;
  - nothing secret: no token, no key.

## 5. Questions

**✅ Q109 (D-120) — How the client finds the server.** The server must listen on some port, and the client must learn it.
*Recommendation:* an ephemeral port picked by the OS, written with the fingerprint and token into `<universe>/server.json` (§4.2). The client reads the data directory for the token anyway (doc 20 §9). Ports never collide, and two Universes could run side by side.
*Alternatives:* (a) a fixed default port, for example 47 100, with a `--port` override, plus the file for the token and fingerprint; (b) a fixed port only.

**✅ Q110 (D-121) — How long the certificate and token live.** Doc 20 §5 and §9 say the server creates them "on first start", which suggests they persist.
*Recommendation:* make both fresh at every server start and publish them only in `server.json`. Then no private key or token sits on disk while the server is down, and nothing needs rotating. Pinning across restarts would add no protection on one PC: whoever can replace the server can also rewrite the file the client trusts. Hosting replaces all of this with real certificates (D-029).
*Alternatives:* (a) persistent certificate and token, created at `create` and stored in the Universe directory (the literal reading of doc 20); (b) persistent certificate, fresh token at every start.

**✅ Q111 (D-122) — Keeping the connection file private on Windows.** On Linux the file gets mode 0600 through `std`. Windows has no mode bits; privacy comes from ACLs. Our code may not use `unsafe` (`CODING_STANDARDS.md`).
*Recommendation:* rely on the per-user ACL that `%LOCALAPPDATA%` already has (the user, SYSTEM and Administrators) and document it. When `NOMAD_DATA` or `--data-dir` points outside the user's profile, log a warning that the token may be readable by others. No new dependency, no `unsafe`.
*Alternatives:* (a) set an explicit owner-only DACL through a maintained Windows ACL crate (a new Windows-only dependency to vet with `cargo deny`); (b) set it with our own Win32 calls (needs the owner's agreement to an `unsafe` exception).

**✅ Q112 (D-123) — The validator's reason on the wire.** `RejectReason::Invalid` carries no detail today (S3), so the client cannot say why an order failed. But the validator's 25 reasons include `UnknownFleet` against `NotOwner`, and `UnknownTarget` against others. A modified client could probe IDs and learn that unseen entities exist, which doc 16 §6 and doc 20 §9 rule out.
*Recommendation:* add a wire enum mirroring the validator's reasons, but collapse "does not exist", "not yours" and "not visible to you" into one `NotFound`, so a rejection reveals nothing about undetected entities. The mapping lives in the server, and the enum lives in `nomad-types` so the client can show messages without depending on `nomad-sim` (D-086). A test checks that every validator reason maps to a wire reason.
*Alternatives:* (a) send the validator's reason as it is (simplest; leaks the existence of entities); (b) keep `Invalid` without detail (no leak; the UI can only say "invalid order").

**✅ Q113 (D-124) — A second connection for the same commander.** The MVP has one human commander. A crashed or restarted client can leave a half-open session until the 30 s idle timeout.
*Recommendation:* the newest authenticated session replaces the old one, which gets `Bye { Replaced }`. The commander stays connected for the simulation (no `Disconnected`/`Connected` pair), so a quick client restart does not start the grace period.
*Alternatives:* (a) refuse the new session until the old one ends (`Bye { AlreadyConnected }`), so a crashed client locks the player out for up to 30 s; (b) allow several sessions per commander (two windows), which N4 must then serve twice.

**✅ Q114 (D-125) — Where the client-side connection code lives.** Doc 20 §3 lists no crate for it. The integration test needs a headless client, and the game client (C7) needs the same code.
*Recommendation:* a new small crate `nomad-link`: the QUIC client, pinning, handshake, reconnects and the channel API (§4.8). It depends on `nomad-protocol` and `nomad-types` only. `nomad-client` uses it in C7, and the server's integration tests use it as a dev-dependency. This amends D-086's crate list.
*Alternatives:* (a) a module inside `nomad-client`, with the integration test in `nomad-client`'s tests (no new crate, but every server test build compiles `wgpu` and `winit`); (b) a `client` feature of `nomad-protocol` (doc 20 says the protocol crate has no networking).

## 6. Tasks, in order

1. **Protocol version 2** (§4.7): the nonce, the rejection reasons (Q112), `SeasonOver` and the new Bye reasons; round-trip and frozen-layout tests, including a version-1 `Hello` against a version-2 server.
2. **Rejection mapping:** the wire reason enum in `nomad-types`; the server-side mapping from `CommandRejection`, with an exhaustive test.
3. **Identity:** certificate and token generation; `server.json` written atomically with user-only permissions (Q109–Q111); removal on clean shutdown; staleness via the lock.
4. **Endpoint and network thread:** the `quinn` server on 127.0.0.1 with `rustls`/`ring`, ALPN `nomad`, no 0-RTT; the event bridge; shutdown order.
5. **Sessions:** handshake and timeouts, Welcome, catch-up progress, commands with envelope checks and result matching, Ping/Pong, Bye; the Q113 rule; limits (§4.6).
6. **Connect and disconnect inputs** (§4.5), including the D-117 change in the simulation thread.
7. **`nomad-link`** (Q114): connect, pinning, handshake, channels, reconnect with backoff.
8. **`nomad-server run`** starts the endpoint; `status` shows the port; logging (§4.9).
9. **Integration tests** (§7), on Linux and Windows in CI.

## 7. Done when

- **CI** is green on both operating systems, with `--locked`, and `cargo deny` passes with the new dependencies.
- **Integration tests** (D-089) use `nomad-link` as the headless client:
  - **In-process** (fast, with the fake clock):
    - handshake and Welcome, with the right commander, Universe and clock;
    - a version-mismatch `Hello` gets `Bye { VersionMismatch }` with the server's version;
    - a wrong token gets `Bye { AuthenticationFailed }`;
    - a valid command gets `Applied { tick }`, with the tick after the one during which it arrived;
    - an invalid command gets the mapped reason, and an order for an unseen entity gets `NotFound` (Q112);
    - commands during catch-up get `CatchingUp`;
    - the rate limit gets `RateLimited`, and a malformed frame gets `Bye { ProtocolViolation }`;
    - the second-session rule (Q113);
    - Ping/Pong returns the nonce;
    - after the season end, commands get `SeasonOver` and the session stays open (D-116).
  - **Journal:** `Connected` and `Disconnected` appear at the right ticks; none fall within catch-up; a D-117 re-catch-up disconnects and reconnects an open session.
  - **Real binary:** start `nomad-server run` on a test data directory, connect through `server.json`, send a command, get the result; stop it with the shutdown path, see `Bye { ShuttingDown }`, restart, and reconnect with the new port and fingerprint.
- **No panics:** fuzz-style tests feed random frames and random message orders to a session; the server answers with `Bye` and keeps running.
- **Security checks:** the endpoint binds 127.0.0.1 only (asserted). On Linux, `server.json` is mode 0600. No log line contains the token.
- **Latency:** on localhost a command's result arrives within one tick plus 20 ms in the in-process test, logged rather than asserted on CI runners.

## 8. New dependencies

All must pass `deny.toml` (MIT, Apache-2.0, ISC, BSD, Zlib, MPL-2.0, Unicode-3.0).

| Crate | Licence | Purpose |
|---|---|---|
| `quinn` (features `runtime-tokio`, `rustls-ring`) | MIT / Apache-2.0 | QUIC (D-087) |
| `rustls` (feature `ring`; no `aws-lc-rs`) | Apache-2.0 / ISC / MIT | TLS 1.3 (D-087) |
| `rcgen` (feature `ring`) | MIT / Apache-2.0 | self-signed certificate |
| `ring` (via the above; used directly for SHA-256 and random bytes) | Apache-2.0 AND ISC | crypto provider, fingerprint, token |
| `tokio`: add the `net` and `time` features to the workspace entry | MIT | `quinn`'s runtime |

- **`ring` instead of `aws-lc-rs`:** `aws-lc-rs` needs CMake (and, on some setups, NASM) for the Windows build. `ring` builds with the C compiler both CI runners already have, as the bundled SQLite does.
- **Licence check:** `ring` 0.17's licence expression is "Apache-2.0 AND ISC", both allowed. `cargo deny` confirms it in task 4.
- **Q111 alternative (a)** would add a Windows ACL crate. It is not listed, because the recommendation needs none.

## 9. Stress test

1. **QUIC on localhost is heavier than a TCP socket.** D-087 chose QUIC so hosting needs no second transport; N3 pays that cost now. The real risk is not speed but setup: certificates and UDP on Windows runners. Task 4 runs on both CI runners early, before the session logic is built on it.
2. **The connection file holds the keys to the session.** Anyone who can read it can play as the user. That is the trust boundary doc 20 §9 already sets (same OS user). Q111 is about not widening it by accident through `NOMAD_DATA`.
3. **The event bridge drops on lag** (N2's bounded subscriber channels). A dropped command result would leave the client waiting. The `Resync` Bye turns that silent loss into a reconnect. The simulation must keep never blocking, so this is the right side to give way.
4. **Fresh credentials at every start** (Q110) mean the client must re-read the file on each reconnect. The link does that anyway, because the port changes too (Q109).
5. **Collapsing reasons to `NotFound`** (Q112) makes some honest errors vaguer, for example ordering a fleet you lost a moment ago. The UI can still say "not found", and the alternative leaks information that fog of war (D-072 to D-076) exists to hide.
6. **The D-117 disconnect injection** (§4.5) changes server code that is being written now. If it lands late, N3's journal tests catch the gap. Without it, a sleeping PC would count as an online player during catch-up, against doc 11 §7.
7. **A new crate** (Q114) amends D-086's list. The alternative of testing through `nomad-client` would put a GPU stack into every server test build, which is worse for both CI time and the separation doc 20 §3 wants.
