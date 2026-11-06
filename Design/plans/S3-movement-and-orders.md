# Work package S3 — Movement, fleets and orders

Status: **Built** (merged on the working branch, PR Zwaliebaba/Nomad#2). Planned and decided as Q92–Q96 → D-101 to D-105. The gaps found while building it were decided as D-112 to D-115 (Q101–Q104). This plan is also the brief for whoever builds S3.

## 1. Goal

Ships exist in the simulation, move in 2D, form fleets, follow the doc 15 §3.4 orders, and travel between systems through gates. All of it is deterministic and covered by tests. This is the first package with real entities, and S4–S7 and the AI build on it.

## 2. Inputs

- **Movement:** doc 15 §3.1–3.4 (classes, movement, cruise mode, fleet orders) and §12 (speeds, as starting values, D-058).
- **Gates and transit:** D-042 (transit cannot be interrupted), D-055 (arrival scatter of 2 km, 10 s jump shroud, 5 s spool-up, contested gates).
- **Galaxy:** doc 10 §8 (frames, ops area), D-099 (metres in the simulation; the 6 h check gains spool-up and tactical legs; at most 15 min in-system).
- **Determinism and IDs:** doc 11 §4–5 (tick phases, determinism), D-049 (`SimId`, `libm`, ordering), D-050 (pure simulation crate), D-030 (`hecs`).
- **AI fairness:** D-077 (one command validator for humans, the Deputy and the AI).
- **Start kit:** doc 14 §12 (3 VX and 2 MULE at the mother station).

## 3. Scope

**In scope:**

- **Ships and fleets** as `hecs` entities.
- **Movement:** 2D kinematics, cruise mode, formations, obstacle avoidance and separation (Q96).
- **Orders:** Move, Station-keep, Patrol, Guard, Dock and Launch, Retreat and Jump. Attack and Attack-move reduce to "close to weapon range and hold" until S5 adds weapons. Rules of engagement are stored only.
- **Fleet management:** create, split, merge.
- **Routing** over lanes.
- **Docking capacity** (Q93).
- **Start-kit spawn.**
- **Unit change:** galaxy positions move from km to metres.
- **Validator:** invariant 3 updated (D-099).
- **Harness:** a fleet demo.

**Out of scope:**

- weapons and damage (S5);
- sensors and visibility (S7; until then all entities count as visible, and the visibility check plugs in at S7);
- building ships (S4);
- the Deputy and AI (A1 onward).

## 4. Design

### 4.1 Rules data

*Proposed (Q95).*

- All unit and movement numbers live in a **rules file**: the agreed starting values, embedded in the build. A harness option can override it for tuning runs.
- At Universe creation, the rules are **copied into `UniverseState`**. A season therefore keeps its rules even if a later build changes the defaults. That fits D-020 ("balance changes land between seasons"), and it keeps snapshot replay exact.

### 4.2 Entities

`hecs::World` inside `UniverseState`. Persistence and ordering use `SimId` only (D-049). Snapshots serialise every component as a table sorted by `SimId`, and the `SimId → Entity` index is rebuilt on load.

| Component | Fields |
|---|---|
| `Ship` | class, owner, fleet |
| `Motion` | position (m), velocity (m/s), heading (rad), mode (tactical or cruise) |
| `Location` | `InSystem(system)` · `Docked(holding)` · `InTransit { lane, to_gate, arrive_tick }` |
| `Shroud` / `SpoolUp` | Deadline tick |
| `Fleet` (its own entity) | owner, members (sorted), formation offsets, order queue, rules of engagement |

### 4.3 Movement

*Derived* from doc 15 §3.3; the parameters are proposed.

- **Tick update** (phase 5, every tick, Δt = 0.1 s):
  - steering seeks and arrives toward the target point;
  - velocity change is capped by the class acceleration;
  - heading turns toward the velocity direction, capped by the class turn rate (Q92);
  - ships decelerate to stop at the target.
- **Formation:** each ship's target is the fleet target plus its offset, rotated with the fleet's heading. The default formation is a compact grid with 150 m spacing, and the fleet moves at its slowest member's speed.
- **Cruise mode:**
  - Allowed **outside every ops area** (radius from Q94) **and while not engaged**.
  - The fleet cruises at its slowest member's cruise speed and cannot fire.
  - Hooks for "taking fire" (S5) and "hostile detected within 20 km" (S7) drop it to tactical speed. In S3 they are fired by tests only.
- **Obstacles and separation** (Q96):
  - Station modules whose height range crosses the ship plane are circular obstacles (footprint radius), avoided by look-ahead steering.
  - Ships keep a soft separation from each other.
  - A uniform grid per system (2 km cells) serves neighbour queries.

### 4.4 Orders

| Order | S3 behaviour |
|---|---|
| Move | To a point, site or gate in the same system; another system's target is rejected (use Jump) |
| Station-keep | Hold a position, or an offset relative to a target |
| Patrol | Loop through waypoints |
| Guard | Stay within the guard radius of an object, following it |
| Attack / Attack-move | Close to the class weapon range and hold; firing arrives in S5 |
| Dock / Launch | Dock at an owned holding with free capacity (Q93). Launch puts ships at the holding's launch point. |
| Retreat | Go to the nearest owned holding by route time, and dock |
| **Jump** | Route over lanes (shortest time: in-system legs at fleet cruise speed, plus lane transit, plus spool-up). Then: move to the gate (within 2 km) → **5 s spool-up** (cancelled if the ship moves away) → **transit timer** (invisible, cannot be interrupted) → **arrival** within 2 km of the destination gate, scattered by the seeded stream, with a **10 s shroud** → continue the route |
| Rules of engagement | Stored on the fleet; used from S5 |

Orders either replace the queue or are appended to it (a queue flag, if the N1 wire shape lacks one).

### 4.5 Validation

*Derived* from D-077. One validator serves every source of commands. It checks:

- ownership;
- that the target exists;
- that the order applies (a docked fleet must launch first);
- reachability;
- the fleet's composition.

Rejections return a reason (doc 20 §5). The visibility check is added at S7.

### 4.6 Galaxy in metres and invariant 3

- Galaxy positions are converted from km to m. This bumps the snapshot format, which is allowed before β (D-099).
- The invariant 3 check gains:
  - 5 s of spool-up per jump;
  - tactical-speed legs inside the origin and destination ops areas, at the WARDEN's 110 m/s.

  Expected margin: the worst case rises by about 6 min, from 4.87 h toward 5 h, still well under the 6 h limit.

## 5. Decisions (formerly open questions)

*Agreed: D-101 (Q92), D-102 (Q93), D-103 (Q94), D-104 (Q95), D-105 (Q96).*

- **Q92 — Acceleration and turn rates** (starting values; doc 15 gives only speeds):

  | Class | Acceleration | Turn rate |
  |---|---|---|
  | VX | 60 m/s² | 90°/s |
  | LANCER | 40 m/s² | 60°/s |
  | WARDEN | 15 m/s² | 20°/s |
  | MULE | 15 m/s² | 25°/s |
  | Raider strike craft | 50 m/s² | 75°/s |
  | HSTL heavy cruiser | 8 m/s² | 10°/s |
- **Q93 — Docking capacity**, counted in ships. The phase-1 fixture shows "DOCKED 14 / 24" with no design behind it. Proposed: the mother-station core 24; other cores 8; each dry dock +6; frigates dock only at holdings with a dry dock.
- **Q94 — Ops-area radius:** confirm 20 km (doc 10 §8, still a working proposal). It drives cruise mode and the tactical view.
- **Q95 — Rules data:** a rules file embedded in the build and copied into the Universe at creation (§4.1), as opposed to constants compiled into code.
- **Q96 — Structures on the ship plane:** the plane at z = 16 cuts through the core spindle (up to z = 33), the pylons and FA-1. Proposed: modules that cross the plane are obstacles that ships steer around, and ships keep a soft separation. The alternative is that ships fly through structures, with visible clipping.

## 6. Tasks, in order

0. **Follow-ups from N1** (implementation fixes within agreed decisions):
   - **Keep `nomad-types` data-only (D-086):** move `ControlState::from_counts`, `majority_threshold` and the deposit value table back into `nomad-sim` as free functions, so the client links no rules. Snapshot hashes must not change.
   - **`Owner` enum:** `Commander(CommanderId)` or `Npc(FactionId)` replaces `Option<CommanderId>` in the types and the view, so raiders can own things (D-014).
   - **Order targets name their system:** Move, AttackMove and Patrol waypoints carry `{ system, position }`, so an order cannot apply in the wrong system after a jump.
   - **Holding launch:** add `Command::Holding { holding, order: HoldingOrder::LaunchReady }` for the phase-1 LAUNCH button (doc 15 §5.3). `Retreat` gets an optional destination, defaulting to the nearest owned holding.

   The protocol is still at version 1 and has never been released, so these shapes may change freely.
1. Rules file and loader; rules stored in the state (Q95). Snapshot format bump.
2. Galaxy positions to metres; update the validator, tests and harness output.
3. `hecs` integration: components, the `SimId` index, canonical snapshot tables. Prove snapshot and restore change nothing.
4. Ships and fleets; start-kit spawn at Universe creation; fleet management commands.
5. Kinematics, formation and cruise mode, with test hooks for the cruise-drop triggers.
6. Obstacles and separation (Q96).
7. In-system orders: Move, Station-keep, Patrol, Guard, Attack (approach), Dock and Launch, Retreat.
8. Routing and Jump: spool-up, transit, arrival scatter, shroud.
9. Command validator for every order.
10. Invariant 3 with spool-up and tactical legs; 200-seed generation still passes.
11. Harness `--fleets`: spawn the start kits, issue scripted and seeded random orders, report arrivals and timings.

## 7. Done when

- **CI** is green on both operating systems: fmt, clippy, build and test, with `--locked`.
- **Determinism:**
  - the same seed with the same order script gives the same hash;
  - snapshot and restore mid-run change nothing;
  - the order in which commands arrive does not matter.
- **Movement tests:**
  - ships reach a target without overshooting more than 1% of the leg;
  - formation spacing holds;
  - a fleet cruises at its slowest member's speed;
  - cruise mode switches off inside ops areas;
  - no ship enters an obstacle footprint.
- **Jump tests:**
  - the route is the shortest by time;
  - spool-up is 5 s and is cancelled by moving away;
  - transit time equals the lane's;
  - arrival lands within 2 km of the destination gate;
  - the shroud lasts 10 s.
- **Validator tests:** every rejection reason is covered.
- **Performance:** in Debug, the harness runs 4 start kits with continuous random orders for 6 simulated hours without errors, and reports tick time.
