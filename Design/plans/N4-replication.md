# Work package N4 — Replication with interest management

Status: **Planned and decided** (Q175–Q180 → D-182–D-187). Written together with `S7-sensors.md`, because N4 sends each client exactly what S7 decides its commander can see. Builds on N3 (sessions, the state stream N3 opens empty, `nomad-link`, protocol version 2) and on S7's `CommanderView`. This plan is also the brief for whoever builds N4. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Goal

A connected client receives its commander's view: a full snapshot when the session starts, then deltas with creates, updates and removes, at rates that follow what the player is looking at. The view holds public data, own assets, sensed entities, last-known entries, lane traffic, and the announcements and notifications addressed to that commander. Nothing the commander has not detected ever reaches the client (doc 16 §6). Timers arrive as deadlines, never as countdowns. The client's view-model store (C7) can be built against a fixed contract, and the client–server integration test in CI checks replication end to end on both operating systems (D-089).

## 2. Inputs

- **Architecture:** doc 20 §4 (threads: the simulation thread publishes per-tick outputs, the network thread sends), §5 (replication: known-entity tracking, creates, updates and removes, the 10 Hz / 2 Hz / 5 s rates, deadlines instead of countdowns, rendering about 100 ms behind with interpolation), §8 (the view-model store; render code reads only the view-model, phase-1 ground rule 3), §9 (bound everything that arrives), §11 (the integration test).
- **Information model:** doc 16 §2 (classes), §6 (what the server sends), §8 (the phase-1 HUD's sources); D-075 (public and private), D-076 (last-known with age), D-074 (lane traffic).
- **Decisions:** D-087 (snapshot then deltas at the doc 16 rates; reliable streams; exact protocol version), D-089 (integration test in CI), D-123 (rejections never reveal unseen entities), D-124 (a new session replaces the old one), D-116 (season over: the session stays open and read-only), D-150 to D-152 (bind, credentials, external servers).
- **Strategic views:** doc 17 §3–§4 (TACTICAL, SYSTEM, SECTOR and GALAXY contents: sites, gates with lane traffic, own, sensed and last-known fleets with age, siege and Reinforced markers, the season summary), §6 (notifications).
- **Plans:** `S7-sensors.md` (`CommanderView`, visibility sets, memory, lane traffic), `N3-transport-and-sessions.md` (§4.1 threads and the bridge, §4.3 streams, §4.4 lifecycle, §4.7 version 2, §4.8 `nomad-link`), the S4 plan §4.8 (own economy views), the S5 plan §4.3 (projectiles live in per-system lists, not as entities).
- **Code:** `crates/nomad-protocol` (`ViewSnapshot`, `ViewDelta`, `ViewChanges`, `EntityChanges`, `EntityEntry`, `Announcement`, `Notification`, the 16 MiB server frame limit), `crates/nomad-server` (`ServerHandle`, the simulation thread, the subscriber channels).
- **Work in flight:** N3 is being built now; S7 is planned with this plan. N4 relies on their plans, not on their exact names.

## 3. Scope

**In scope:**

- **View extraction** from `CommanderView` into wire types, for each connected commander (Q175).
- **Per-client replication state:** what the client knows, per collection, and the diff into upserts and removes.
- **Snapshot, then deltas,** on the state stream N3 opens; resync.
- **Rate tiers** by area (Q176) and the client's report of what it views (Q179).
- **The view contract:** typed entries per entity kind (Q177), deadlines, the tick-to-UTC mapping, combat effects (Q178).
- **Announcements and notifications** delivered on the state stream from the simulation's events.
- **Protocol version 3** with the new messages and entry types.
- **Tests:** the snapshot–delta equivalence test, the "never reveal" oracle test, and the integration test extended with replication on both operating systems.

**Out of scope:**

- **What a commander can see** (S7). N4 never decides visibility; it only projects and sends.
- **The client's view-model store, interpolation and rendering** (C7, C8). N4 defines what they receive.
- **The away report's 7-day history** (U5) and notifications outside the client (doc 17 §6, out of the MVP).
- **Unreliable datagrams and bandwidth optimisation for hosting** (doc 20 §13 items 2 and 5).

## 4. Design

### 4.1 Where the view is computed

*Proposed (Q175).*

- **The simulation thread extracts, the network thread diffs.** After each step, for each connected commander whose tiers are due (§4.4), the server's simulation thread builds a `ViewExtract` through S7's `CommanderView`: owned wire-shaped entries for the due scope. It sends the extract over the existing subscriber channel. The network thread keeps each session's known state, diffs, frames and sends.
- **Why:** visibility has one implementation (S7's), used by the AI and by replication alike, so the AI and the player provably see the same things (D-027). The cost scales with connected clients (one human in the MVP), not with the size of the Universe. The network thread needs no mirror of the state.
- **Read-only and outside the step:** extraction reads the state after `step` returns and never changes it, so it is not journaled and cannot affect determinism. `nomad-sim` stays free of protocol types (D-086): the mapping to wire types lives in `nomad-server`.
- **Amends doc 20 §4,** which says the simulation thread publishes "change sets, visibility sets". It publishes per-session extracts instead.

### 4.2 Collections and the per-client state

*Derived* from doc 16 §2, §6 and the N1 view types.

- **Collections**, as in the N1 `ViewSnapshot`: `public` (topology, holdings and their owners), `control` (system control states), `own`, `sensed`, `last_known`; plus two new ones: `lane_traffic` (observed departures and own transits) and `standings` (season summary, S8; empty until then).
- **Known state per session:** for each collection, the last entry sent for each `SimId`, and the scope (system) it was sent in.
- **Diff:** for a refreshed scope, an entry that is new or changed is upserted; a known entry in that scope that is no longer in the extract is removed from that collection. Moving to a different rate tier never removes anything: entries leave a collection only when they leave it in the simulation (lost from view, destroyed, expired).
- **An entity can be in several collections** (a holding is public and sensed; a fleet that left view is in `last_known`). Each collection says what is known from that source; the client merges (C7).

### 4.3 Snapshot, deltas and resync

*Derived* from doc 20 §5 and D-087; the consistency check is *Proposed*.

- **Snapshot:** right after `Welcome` (N3 §4.4) the session receives one `ViewSnapshot` of every collection, in one frame. A stage-2 view (topology of 18 systems, about 100 own entities, a few hundred sensed and remembered ones) is well under 1 MiB, far below the 16 MiB limit. A test asserts that a stage-2 stress view stays under 4 MiB.
- **Deltas:** `ViewDelta { tick, since, changes }`, where `since` is the tick of the previous view message on this stream. The client checks the chain; a gap means its view is wrong, and it reconnects. Reliable, ordered streams make gaps impossible in normal operation; the check catches bugs and N3's `Bye { Resync }` path.
- **Resync:** a reconnect, N3's `Resync`, or a session replaced under D-124 always starts again with a snapshot. There are no periodic full snapshots.
- **During catch-up** (doc 20 §7) the session receives only `CatchUp`; the snapshot follows when the server goes live. **After the season ends** (D-116) deltas continue (the Universe no longer changes, so they are empty), and the view stays readable.

### 4.4 Rate tiers and focus

*Agreed* rates (D-087, doc 16 §6, doc 20 §5); their mapping to views is *Proposed* (Q176).

| Tier | Scope | Rate | Contents |
|---|---|---|---|
| **Focus** | The system the player is viewing, in TACTICAL or SYSTEM (Q176) | 10 Hz (every tick) | Own, sensed and last-known entries in that system; combat effects (§4.6) |
| **Presence** | Every other system where the commander has assets, sensors or control | 2 Hz (every 5 ticks, after the sensors pass) | Own, sensed and last-known entries in those systems |
| **Galaxy** | Everything else | Every 5 s | Public changes, control states, lane traffic, own fleets in transit, last-known changes outside the other tiers, standings |

- **Announcements and notifications** are not tiered: each is sent as soon as the simulation produces it.
- **Focus** comes from the client (Q179). Until the client sets it, the focus is the system of the commander's mother station.
- **Focus never widens what is sent,** only how often: every tier extracts through the same `CommanderView`, so a client that lies about its focus learns nothing new.

### 4.5 The view contract

*Proposed (Q177);* timers *Agreed* (doc 20 §5).

- **Typed entries per kind** replace the minimal `EntityEntry`:
  - **Fleet:** owner, system, position, heading, velocity, mode (cruise or tactical), composition (count and hull % per class), shrouded flag. For own fleets also: orders queue, rules of engagement, engaged-until deadline, cargo.
  - **Ship** (own fleets, and detected members of sensed fleets): class, hull %, fleet.
  - **Holding:** owner, site, kind (holding or outpost), modules (kind, slot, hull %, disabled, constructing until), dome sectors (strength %, weak or breached) when S5b exists, exposure window (public, S6), Reinforced or siege deadlines (announced, S6). For own holdings also: queue, power report, buffers, deposits, built-until deadlines.
  - **Last-known** entries carry the same shape as S7's memory entries (Q170) and `seen_at`.
  - **Lane traffic:** fleet, owner, composition as seen, origin, destination, arrival deadline.
  - **Own economy:** pool and cap per resource, income over the last hour, command used and cap (S4 §4.8).
- **Positions in metres in the system frame,** with velocity, so the client can interpolate in the focus tier (about 100 ms behind, doc 20 §5) and extrapolate between 2 Hz updates in the presence tier.
- **Deadlines, never countdowns** (doc 20 §5, doc 11 §3): every timer is an absolute tick. The client turns ticks into Universe time with `Welcome`'s `SeasonClock` (N3 §4.4).
- **Private stays private** (D-075): orders, cargo, queues, doctrine and stockpiles appear only in `own`.

### 4.6 Combat effects

*Proposed (Q178).*

- **Projectile events, not projectile state:** in the focus tier, each delta lists projectiles fired since the previous delta (kind, origin, velocity, fired-at tick, owner) and projectile ends (hit or expired, with the tick and position). Flak bursts and hits name their target when it is visible.
- **Filtered like everything else:** a projectile is sent only if its shooter or its target is own or sensed for this commander. A shot from an undetected ship at an undetected ship is never seen.
- **The client simulates straight flight** between the events; doc 15 projectiles fly straight (torpedoes do not home), so the client's picture matches the simulation's. Hits and destructions also arrive as entity updates (hull %, removal), which stay authoritative.

### 4.7 Messages and protocol version 3

*Derived:* the N1 rule increments `PROTOCOL_VERSION` for every message change. N4's changes land as one step, 2 → 3.

- **Server → client:** `ViewSnapshot` and `ViewDelta` with the new collections and typed entries (§4.2, §4.5); `ViewDelta` gains `since`; `Announcement` and `Notification` carry what S5 and S6 produce.
- **Client → server:** `SetFocus { focus }` (Q179), appended as a new variant (the frozen variant 0 stays `Hello`). It is rate-limited (at most 10 per second) and never journaled. A focus naming an unknown system is ignored.
- **Limits:** a server frame stays under the 16 MiB limit; the snapshot size test (§4.3) guards it.

### 4.8 Threads and cost

*Derived* from doc 20 §4 and N3 §4.1.

- **Simulation thread:** extraction per due tier, through `CommanderView` (§4.1). The harness measures extraction time per tick for one session at stage 2, focus on a 100-against-100 battle; target under 1 ms per tick in a release build.
- **Network thread:** diffing and framing per session; never touches the state.
- **Bridge:** extracts travel on the existing subscriber channel. If a session falls behind (N3 §4.1), it gets `Bye { Resync }` and reconnects into a fresh snapshot.
- **Bandwidth:** localhost only in the MVP (D-085, D-150); measured, not optimised. The harness reports bytes per second per session in the stress battle.

## 5. Questions

Numbered Q175–Q180; each has a recommendation and alternatives. S7's questions (Q166–Q174) are in `S7-sensors.md`.

- **✅ Q175 (D-182) — Where each client's view is computed.** Doc 20 §4 has the simulation thread publish "change sets, visibility sets" for the network thread to filter.
  *Recommendation:* after each step, the simulation thread extracts each connected commander's due view through S7's `CommanderView`, and the network thread only diffs and sends. One visibility implementation serves the AI and replication, and the cost scales with connected clients, not the Universe. *Alternatives:* the simulation publishes a Universe-wide change set plus visibility deltas, and the network thread keeps a mirror and filters (doc 20 §4's wording; a second visibility filter to keep identical); the simulation keeps per-client known sets and emits ready deltas (puts session state into the simulation thread).
- **✅ Q176 (D-183) — What the 10 Hz tier covers.** Doc 16 and doc 20 say "the ops area being viewed". In SYSTEM view, fleets cross the whole system and need smooth motion too.
  *Recommendation:* the focus tier is the whole system being viewed, in TACTICAL or SYSTEM; other systems with presence at 2 Hz; the galaxy summary every 5 s. On localhost the extra traffic does not matter. *Alternatives:* exactly as written, only the viewed ops area at 10 Hz and the rest of that system at 2 Hz; everything at 10 Hz in the MVP (localhost only), tiers added for hosting.
- **✅ Q177 (D-184) — How rich view entries are.** The N1 `EntityEntry` holds only kind, owner, location and last-seen; every added field bumps the protocol version.
  *Recommendation:* typed entries per kind now, carrying doc 16's identification (class, owner, hull %, fleet membership), position, heading and velocity, and in `own` the private details that the β views need (orders, queues, power, cargo, economy; U1–U3). Fields for later packages (S6 deadlines, S8 standings) are added in their packages. *Alternatives:* a generic key–value property bag (no version bumps, no type safety); minimal entries extended one U-package at a time (a version bump each time).
- **✅ Q178 (D-185) — How combat effects reach the client.** Projectiles are not entities (S5 §4.3), but TACTICAL must draw shots, hits and flak.
  *Recommendation:* in the focus tier, events for projectiles fired and ended since the last delta, sent only when the shooter or target is own or sensed; the client simulates the straight flight between them. *Alternatives:* every projectile's position in every update (simple, heavier); only hit and destruction events, with shots drawn as client-side guesses (lightest, least faithful).
- **✅ Q179 (D-186) — How the client tells the server what it views.** The rates depend on the focus.
  *Recommendation:* a `SetFocus { focus }` message (TACTICAL site, SYSTEM, SECTOR or GALAXY), rate-limited, not journaled; it only changes rates, never visibility. Until the client sends one, the focus is the mother station's system. *Alternatives:* the server infers the focus from the last area the commander gave orders in; the client subscribes to an explicit list of systems at 10 Hz.
- **✅ Q180 (D-187) — Delta consistency.** QUIC streams are reliable and ordered, so deltas cannot be lost in normal operation.
  *Recommendation:* each delta carries `since`, the tick of the previous view message; the client reconnects into a fresh snapshot if the chain breaks. No periodic full snapshots. *Alternatives:* no check, trusting the stream; a full snapshot every 60 s as a self-healing net (costs bandwidth, hides bugs).

## 6. Tasks, in order

1. **Protocol version 3:** typed entries (Q177), the new collections, `ViewDelta.since` (Q180), projectile events (Q178), `SetFocus` (Q179); round-trip and malformed-frame tests; the snapshot-size test at stage-2 scale.
2. **Extraction:** `ViewExtract` from `CommanderView` per collection and scope, in `nomad-server`; mapping to wire types; deadlines as ticks.
3. **Simulation-thread scheduling:** per session, which tiers are due on each tick (§4.4), extraction after `step`, sending over the subscriber channel; focus from `SetFocus`.
4. **Network-thread replication:** per-session known state, the diff, snapshot on session start and after resync, deltas with `since`; announcements and notifications as they arrive.
5. **Combat effects:** projectile events in the focus tier, filtered by visibility (needs S5a).
6. **Tests:** the equivalence test, the oracle test, rates and focus, catch-up and season-over behaviour (§7).
7. **Integration test:** extend N3's real-binary test with replication on Linux and Windows.
8. **Harness:** extraction time and bytes per second per session at stage 2 in the stress battle.

## 7. Done when

- **CI** is green on both operating systems, with `--locked`; `cargo deny` passes.
- **Equivalence:** for scripted runs (movement, combat, fog transitions, memory expiry, lane traffic), the view the client builds from the snapshot plus every delta equals a fresh snapshot taken at the same tick, at every 50th tick.
- **Never reveal (oracle):** in a two-commander scenario, every entity, field and projectile event in every message to commander A is present in A's `CommanderView` at that tick; nothing private of B ever reaches A.
- **Rates:** focus entries update every tick, presence entries every 5 ticks, galaxy entries every 50; changing the focus changes rates without removing anything.
- **Lifecycle:** catch-up sends only `CatchUp` and the snapshot follows going live; a replaced or resynced session starts with a snapshot; after the season ends the view stays readable.
- **Integration test** (D-089): the headless client connects to the real `nomad-server` on Linux and Windows, receives the snapshot, orders a fleet and sees it move in deltas, sees a scripted hostile appear in `sensed` and then move to `last_known`, restarts the server, and reconnects into a snapshot equal to its rebuilt view.
- **Performance:** extraction under 1 ms per tick for one session in the stage-2 stress battle (release build); bytes per second reported.

## 8. Stress test

1. **Extraction on the simulation thread** is cheap for one human, but for hosting with 100 sessions it multiplies. Moving it to a worker with a cloned or double-buffered state is the hosting path (doc 20 §13 item 5); the extract types do not change.
2. **Rich typed entries (Q177)** make the first protocol version big, and any field missing for a later U-package still costs a version bump. The exact-match rule (D-087) makes bumps cheap while client and server ship together.
3. **The oracle test is the real safeguard** for doc 16 §6. Every field added later must go through `CommanderView`, or the test fails. A field taken straight from `UniverseState` in the extraction code would bypass S7; extraction therefore only accepts `CommanderView`.
4. **Projectile events (Q178)** depend on the client simulating flight the same way. Straight flight with no homing makes that safe; any future homing weapon would need position updates instead.
5. **The 2 Hz presence tier** looks choppy without extrapolation. Velocity in every entry lets C7 extrapolate; C7 must do so for the SECTOR and other-system views.
6. **A 1-tick extraction delay** (state after step, sent before the next tick) adds up to 100 ms of latency on top of the client's 100 ms interpolation delay. On localhost that is acceptable; hosting will feel it.

## 9. Effect on other documents

- **Doc 20 §4:** the simulation thread publishes per-session view extracts instead of Universe-wide visibility sets (Q175).
- **Doc 20 §5 and doc 16 §6:** the focus tier covers the viewed system (Q176); `SetFocus` joins the client messages; protocol version 3.
- **Doc 17:** none; the contract carries what §3–§4 need.
- **Doc 30:** N4 ✅ when done; C7 builds its view-model store on §4.5.
