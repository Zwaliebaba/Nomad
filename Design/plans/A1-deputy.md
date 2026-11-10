# Work package A1 — The Deputy, and the AI architecture

Status: **Planned and decided** (Q200–Q212 → D-199 to D-211). **Built:** A1a (tasks 1–4: the `nomad-ai` crate, outbox, gate, inbox, doctrine, direct control, rebuilding, convoy rerouting and holding) and A1b (tasks 5–9: threat, alert, abilities, engagement, roles, convoy evacuation, harness `--deputy`, the budget). Written together with `A2-strategist-economy.md` and `A3-raider-director.md`, which build on the architecture in §4: the `nomad-ai` crate, the command outbox, the competence gate, the event inbox, phases and budgets, determinism, and presence before A5. Builds on S4 (built), S5a (built), S5b (abilities, turrets, sectors; built), S6a (presence, grace, order lapse; built) and S7 (`CommanderView`; planned). This plan is also the brief for whoever builds A1. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval. *Build status (2026-11-09):* A1a built (§12), A1b built (§13).

## 1. Goal

Every commander, human or AI, has a Deputy that always executes its doctrine (D-013, PR-4), within the authority of doc 12 §5.1 (D-016):

- it defends holdings and uses abilities on their triggers;
- it launches, recalls and retreats fleets;
- it keeps convoys running and out of harm's way;
- it rebuilds losses from the replacement list within a daily budget.

It never attacks, expands, claims, changes doctrine or starts a project (PR-3). It sees only `CommanderView`, acts only through ordinary commands and the single validator (D-077), reacts no faster than its competence cap (doc 12 §5.4, D-079), and stays within the tick budget (D-083 item 7). A1 also lays the foundation A2 and A3 use: one crate, one view, one command path and one budget for all three AI layers.

## 2. Inputs

- **The Deputy:**
  - doc 12 §5: authority (D-016), doctrine (§5.2), connection rules (§5.3), competence (§5.4);
  - doc 13 §4: the Deputy's rules, still *Proposed*;
  - doc 13 §9: doctrine presets (D-082);
  - D-013 and PR-3 to PR-5;
  - RK-10 (doc 04, decided): a direct order overrides doctrine until the player releases the units, or until they disconnect and the grace expires; the retreat threshold applies under a direct order only if the player opts in for that order; the HUD shows direct control.
- **The order lapse:** D-160 to D-162. Direct fleet orders lapse after the 120 s grace and fleets fall back to doctrine; economy queues and convoy routes are doctrine and continue.
- **The AI stack:**
  - D-077: three layers, `CommanderView` only, one validator, inside the deterministic step;
  - D-078: the Strategist (A2, A4);
  - D-079: difficulty changes competence only (latency, command rate, interval, noise);
  - D-080: seeded schedules, every AI offline during catch-up;
  - D-081: the raider director (A3);
  - D-083: acceptance, including the 5 ms budget and build-time plus runtime fairness checks.
- **Catch-up:** D-047 (everyone offline; no new raids), D-117 (sleep catch-up), D-171 (catch-up minutes count as offline).
- **Information:**
  - D-173 to D-181 and the S7 plan §4.7: `CommanderView` by information class, per-system indices, and a build-time fairness check;
  - D-075: online status and doctrine are private.
- **Combat:**
  - S5a (built): `CombatEvent::ShipDamaged { by }`, auto-targeting (D-144), rules of engagement (D-145), engagement (D-114);
  - the S5b plan: `HoldingOrder::Ability`, `TurretsHold`, sector strengths, SHLD SURGE (D-147), FLAK SCREEN (D-148);
  - D-052: the counter triangle.
- **Economy** (S4, built): build queues and commands, rally, power profiles, convoys (D-134), `CommanderEconomy` with `income_last_hour`; D-055 (contested gates).
- **Simulation:**
  - doc 11 §4.1: phase 3 Strategist, phase 4 Deputy at 2 Hz, AI output as commands that apply on the next tick;
  - doc 11 §4.3: budget (mean tick ≤ 30 ms at full scale; fast-forward ≥ 500× at stage 1);
  - doc 11 §5 and D-049: determinism;
  - doc 11 §9: "the Deputy and the AI (as modules or a sibling `nomad-ai` crate)";
  - D-050, D-104.
- **Code:**
  - `crates/nomad-sim`: `step` (phases 1, 2, 5, 7, 8 so far), `execute` and `validate_command(state, issuer: Owner, …)`, `FleetView` (order, queue, engagement), `TickOutputs` events, `RngStreams`/`StreamKey`, `catching_up`;
  - `crates/nomad-types`: `Command` ("doctrine commands arrive with their work package"), `Owner`, `FactionId`;
  - `crates/nomad-harness`;
  - `crates/nomad-server`: `HUMAN_COMMANDER = CommanderId(1)`, and one `--difficulty` for all AI commanders.

## 3. Scope

**In scope:**

- **Shared (§4):**
  - the `nomad-ai` crate and its view trait (Q200);
  - AI memory in the state;
  - the outbox and the competence gate (Q201, Q202);
  - the event inbox (Q203);
  - phases, staggering and budgets, with their measurement (Q204);
  - AI random streams;
  - the fairness checks;
  - presence before A5 (Q205);
  - AI parameters in the rules file (Q212);
  - the dependencies on S5b, S6 and S7, and the build order (Q206).
- **The Deputy (§5):**
  - doctrine data and commands, with presets and overrides per holding, per fleet and for the economy;
  - direct control and release (RK-10), with the per-order retreat opt-in;
  - doctrine as the fallback for the order lapse (D-162);
  - threat assessment and alert;
  - alert actions: power profile, garrison launch, abilities, evacuation;
  - engagement: target priority, leash, retreat;
  - fleet roles (Q208);
  - rebuilding from the replacement list within the budget (Q209);
  - convoys: evacuation, rerouting, holding (Q211);
  - Deputy events for the away report;
  - harness `--deputy`.

**Out of scope:**

- The Strategist (A2, A4) and the raider director (A3).
- Personalities, per-commander difficulty and schedule tuning (A5; schedules may move into A2, Q205).
- The doctrine editor and the Deputy status display (U4).
- Carrying doctrine presets over between seasons (D-035).
- Derelict and vacation behaviour (D-038). Alliances (D-040).

## 4. Shared AI architecture (A1, A2, A3)

### 4.1 The `nomad-ai` crate and the view

*Proposed* (Q200); the principle is *Agreed* (D-077, doc 16 §7, D-083 item 8).

- **The problem with S7's check.** In Rust a child module can name every item its ancestors can, so an `ai` module inside `nomad-sim` can always name the crate's `UniverseState`. A compile-fail test only shows what *outside* code cannot do. S7's "module visibility plus a compile-fail test" (S7 plan §4.7) therefore cannot guarantee D-083 item 8.
- **The proposal:** a new crate, `nomad-ai`, holds all AI logic: the Deputy, the Strategist and the raider director.
  - It depends on `nomad-types` only (plus `serde`, `libm` and `rand_chacha`).
  - It defines the trait `View`: the read-only queries by information class from the S7 plan §4.7, with small value types that it defines itself or that `nomad-types` already has.
  - `nomad-sim` depends on `nomad-ai`, implements `View` for S7's `CommanderView`, and calls the AI from its phases. The calls are monomorphised, so there is no dynamic dispatch.
  - Because of that dependency, `nomad-ai` can never depend on `nomad-sim` (Cargo refuses cycles), so it cannot name `UniverseState` at all. The build itself is the fairness check.
- **Purity:** `nomad-ai` follows `nomad-sim`'s rules (D-049, D-050, `CODING_STANDARDS.md` §8): no clock, I/O or threads; sorted iteration; `libm`; no `HashMap` iteration.
- **Amends** D-086's crate list (doc 20 §3), as D-125 did, and uses doc 11 §9's sibling-crate option. D-050 still holds: the AI runs inside `step`.
- **Cost:** plain data the AI must read moves to `nomad-types` (for example `Stock`), or is mirrored by a small `nomad-ai` type. Rules values the AI needs (costs, ranges, class stats) are `View` queries.

### 4.2 AI memory in the state

*Derived* from D-048 and D-049: decisions depend on memory, so memory must be in the snapshot.

- `UniverseState` gains `ai: nomad_ai::Memory`, a serde type. It holds per commander the Deputy's and the Strategist's memory, and per NPC faction the director's.
- It contains:
  - plans and commitments;
  - alert timers;
  - last-read ticks;
  - budget ledgers;
  - the AI's random streams (§4.6).
- It is in snapshots and the state hash. Snapshot and restore anywhere must give identical decisions.
- **During the AI phases** the step moves the memory out of the state, builds each `CommanderView` over the state, calls the AI with `&view` and `&mut memory`, and moves the memory back. The view stays strictly read-only.

### 4.3 The command outbox and the competence gate

*Agreed* principle (doc 11 §4.1: AI output is commands applied on a later tick; doc 13 §4 item 6; D-079); the mechanics are *Proposed* (Q201, Q202).

- **The outbox:** AI layers never call `execute`. They append `AiCommand { owner, layer, not_before, command }` to a per-owner outbox in the state.
  - `owner` is a commander, or an NPC faction (`Owner::Npc`).
  - `layer` is Deputy, Strategist or Director.
- **Applying:** in phase 1 of a later tick, after the tick's external inputs (players, operator), due AI commands are applied in `(owner, layer, sequence)` order. They go through the same `execute`, and so the same `validate_command`, as a human command (D-077).
- **Direct control wins** (RK-10): a Deputy command for a fleet under direct control is dropped when it is applied, unless it is the retreat the direct order opted in to (§5.2). A human's command and a Deputy's command in the same tick therefore never fight.
- **Latency** (Q202): `not_before` = the tick of the triggering event + the latency. The latency is 10, 6 or 3 ticks (Easy, Normal, Hard, D-079), and 3 ticks for a human's Deputy (doc 12 §5.4: 300 ms). The Strategist's trigger is its evaluation tick.
- **Rate** (Q202): at most the commander's command rate applies in any 10 consecutive ticks, across all its AI layers: 1, 2 or 4 per second (D-079), and 4 per second for a human's Deputy. Due commands wait in the outbox, oldest first, with Deputy commands before Strategist commands at the same `not_before`.
  - Commands still waiting after 10 s are dropped and counted. The AI re-evaluates long before then anyway.
  - A raider group drains at Normal's values (A3 §4.6).
- **Results:** each AI command's outcome (applied, rejected with its reason, dropped) is recorded for its layer in the inbox (§4.4), so plans see failures. AI outcomes stay out of `TickOutputs.commands`, from which the server answers clients (N3).

### 4.4 Events: the inbox

*Proposed* (Q203); doc 16 §7 and the S7 plan §4.7 put "announcements and notifications addressed to this commander" in the view.

- The AI runs in phases 3–4, but most events arise in phases 5–8, so a layer needs the events of earlier ticks.
- **The inbox:** per owner, in the state, filled at the end of each step from that tick's outputs. It holds only what the owner may know:
  - damage to its own ships, modules and dome, coalesced per own entity and attacker owner per tick, with the attacker's fleet if it is sensed;
  - its own losses;
  - its fleets' order events (completed, dropped, target lost, D-180);
  - its economy events;
  - S6 announcements addressed to it;
  - public season events (S8);
  - the outcomes of its own AI commands.
- Entries are kept for 60 s, the alert window (doc 13 §4). Each layer remembers its last-read tick.
- The inbox is read through `View`. It is part of the commander's own information, never another commander's.

### 4.5 Phases, staggering and budgets

*Agreed* rates (doc 11 §4.1, D-079) and budget (D-083 item 7, doc 11 §4.3); the placement and the numbers are *Proposed* (Q204).

| Phase | Layer | Rate | Stagger |
|---|---|---|---|
| 1 | Apply due AI commands (§4.3) | Every tick | — |
| 3 | Strategist (A2, A4), only while its commander is online | Every 30 / 15 / 10 s by difficulty (D-079) | By commander |
| 4 | Deputy, per holding and per fleet | 2 Hz | By `SimId` mod 5 |
| 4 | Raider group control (A3) | 2 Hz per raid group | By `SimId` mod 5 |
| 2 (timer) | Raid launches (A3, a rule) | One timer per haven | — |

- **What the AI sees:** the state after this tick's phases 1–2, and the visibility sets of the last sensors pass (at most 0.5 s old, D-173).
- **Budgets** (release build, stage 1):
  - **Strategist:** a mean of 5 ms or less per evaluation (D-083). Each evaluation's work is bounded by deterministic counters (candidates scored, plans advanced), never by elapsed time, which the simulation must not read.
  - **Deputies and raider control:** together at most 25% of a stage-1 tick at the doc 11 §4.3 fast-forward target of 500×, which is a 0.2 ms tick. That leaves about 50 µs per tick on average. Catch-up runs every Deputy and no Strategist (D-080), so this is the budget that keeps a night's catch-up at about a minute (doc 11 §7).
  - Live, a 10 Hz tick has 100 ms, so neither budget is tight in real time.
- **Cheap when calm:** a Deputy evaluation first checks the system's visibility index (S7) and the inbox. With no hostile in the system and no new events, it returns at once. That is the common case.
- **Measurement:** `nomad-ai`'s entry points are plain functions of `&impl View`, so the harness, which may read the clock, times them directly on real states in a benchmark mode. It also compares whole-tick times with the AI on and off.

### 4.6 Determinism

*Derived* from D-049 and doc 13 §2 ("seeded per-commander streams").

- **Random streams:** one stream per (commander, layer) and one per (haven, raids). They are seeded by doc 11 §5's derivation from `(season_seed, scope, purpose)`, with new purposes `AI_DEPUTY`, `AI_STRATEGIST`, `AI_SCHEDULE`, `AI_PERSONALITY` and `RAIDS`. The streams live in AI memory (§4.2), so draws never depend on another commander's activity.
- **Order:** commanders in `CommanderId` order, entities in `SimId` order, systems in `SystemId` order.
- **Scores** are integers (per mille), with an explicit tie-break on `SimId`, so noise and ties are exact. Geometry uses `f64` with `libm`, as the rest of the simulation does.
- **Not journaled:** AI decisions and schedules (doc 13 §2, doc 11 §6). Replaying the external inputs reproduces them.

### 4.7 Presence before A5

*Agreed:* while online the Strategist plays; while offline only the Deputy runs; every AI is offline during catch-up (D-080); harness runs may override schedules (doc 13 §6). How to get there before A5 is *Proposed* (Q205).

- S6a gives AI presence through the harness operator input `SetPresence { commander, online }`, journaled, until A5 supplies schedules (S6 plan §4.1).
- **What the AI needs:** an expansion trip must finish while its commander is online, because claiming is the commander's own initiative (PR-3) and its direct orders lapse 120 s after it goes offline (D-162). A Strategist that cannot see when its session ends cannot fit plans into it (A2 §4.8). A4's siege timing rule (D-078) needs the same knowledge.
- **Recommendation (Q205):** move D-080's seeded schedule generator into A2. It runs in the simulation from the seed, without a journal, and drives S6a's presence and the AI's initial window (D-157). `SetPresence` stays as a harness override ("always online", "always offline"). A5 keeps personalities, difficulty and schedule tuning.

### 4.8 Fairness

*Agreed* (D-027, D-077, D-083 item 8, doc 16 §7).

- **Build time:** the crate graph (§4.1), plus a unit test in `nomad-ai` that reads its own manifest and fails if it lists `nomad-sim`, `nomad-server` or `nomad-protocol`.
- **Runtime:** AI commands reach the state only through the outbox and `execute`. Per layer, the simulation counts commands issued, applied, rejected (by reason) and dropped. A test feeds the same invalid command through a human input and through the outbox, and expects the same rejection. The harness reports the counts.
- **No identity bias** (D-078):
  - `View` does not say whether another commander is human or AI;
  - it does not show another commander's presence (private, D-075) or doctrine (private, D-075).

### 4.9 AI parameters in the rules file

*Proposed* (Q212).

- A new `ai` section joins the rules file (D-104). It holds:
  - the doctrine presets (doc 13 §9, D-082);
  - the difficulty table (D-079);
  - the human Deputy's competence (Q202);
  - inbox retention and command expiry;
  - the Strategist's hysteresis and noise (A2);
  - the built-in blueprints (A2);
  - the raid parameters (A3).
- They are copied into the Universe at creation, so a season keeps its AI and replay stays exact, and A6 can tune them through `--rules` without a rebuild. The rules format version increases.

### 4.10 What A1–A3 need, and the build order

*Derived* from the plans; the order is *Proposed* (Q206).

| Need | Comes from | Used by |
|---|---|---|
| `CommanderView`: own assets, sensed entities per system, public data, last-known memory, lane traffic | S7 task 5 | A1b, A2, A3 |
| NPC observers with visibility sets but no memory (D-181) | S7 tasks 2, 5 | A3 |
| The inbox (Q203) on S5/S6 events | This plan, inside S7's view | A1, A2, A3 |
| Presence, the 120 s grace, and the lapse to doctrine (D-160 to D-162) | S6a | A1 (fallback), A2 (online) |
| Public windows and `breachable_from`, siege announcements | S6a, S6b | A2 (risk), A4 |
| The contested-gate flag (D-055) | S6b §4.7, if S4 lacks it | A1 convoys |
| The outpost exception (D-054) and peripheral outcomes by window | S6b | A3 |
| SHLD SURGE, FLAK SCREEN, turrets, sector strengths | S5b | A1b |
| Module damage, disabled modules | S5b | A1 (rebuild), A3 |
| Standings, the season clock | S8 | A2, A4 |
| Spawning NPC fleets | A3 (a new rule) | A3 |

Proposed order towards α (Q206):

| Package | Starts after | Runs in parallel with |
|---|---|---|
| S5b, S6a | (being built) | each other |
| S8 | now (S8 plan Q197) | S5b, S6a |
| S6b | S5b (D-172) | S7, A1a |
| S7 | S6a | S6b, A1a |
| A1a | S6a | S6b, S7 |
| A1b | S7 and S5b | S6b |
| A2 | A1a and S7 (its α runs also need A1b) | A3 |
| A3 | S6b, S7 and A1a | A2 |
| α runs (A3 §5) | A1b, A2, A3, S8 | — |

The two longest chains are S5b → S6b → A3 and S6a → S7 → A1b → A2.

> *Note (2026-11-07):* the owner adopted this order as D-205. S5b, S6a and S8 are built, so S6b, S7 and A1a come next.

- **A1a**, which needs no sensing, starts after S6a, in parallel with S6b and S7. It covers:
  - the crate, view trait and memory;
  - the outbox, gate and inbox plumbing;
  - doctrine and its commands;
  - direct control;
  - rebuilding;
  - convoy rerouting and holding at contested gates.
- **A1b** (threat, alert, engagement, abilities, convoy evacuation: everything that needs sensed hostiles) follows S7 and S5b.
- **A2** follows A1a and S7. Its α runs need A1b.
- **A3** follows S6b, S7 and A1a, in parallel with A2.
- This amends D-095's strictly serial "S6b → S7 → A1 → A2 → A3". S7 depends on S3, S4 and S5a (S7 plan, header), not on S6b. A3 and A2 touch different layers.

## 5. The Deputy

### 5.1 Doctrine

*Agreed* content (doc 12 §5.2, D-082, D-092: set per holding, per fleet and for the economy); the data shape is *Proposed*.

- **Doctrine is player input,** stored in the simulation state per commander, and private (D-075). The Deputy reads it through `View` as own information; N4 sends it to its owner (U4).
- **The types** live in `nomad-types`, because they travel on the wire:
  - `DoctrinePreset` — Fortress, Balanced or Trader;
  - `HoldingDoctrine` — defended-zone radius, rules of engagement in the zone, alert power profile, SHLD SURGE threshold, FLAK SCREEN light-hostile count (or never), evacuation radius, rebuild modules (Q209);
  - `FleetDoctrine` — role (Q208), leash, retreat threshold, repair at dock;
  - `EconomyDoctrine` — the replacement list, its budget share, the garrison share (Q210).
- **Presets** come from the rules file (Q212), with doc 13 §9's values (D-082). A commander's doctrine is a preset plus optional overrides per holding and per fleet. An override set to "none" follows the preset again. Balanced is the default (D-082).
- **Commands:** `Command::Doctrine(DoctrineCommand)` with `SetPreset`, `SetHolding`, `SetFleet`, `SetReplacement` and `Release { fleet }`, each with validator reasons and tests.
  - The validator refuses doctrine commands from the Deputy layer: changing doctrine is the commander's own initiative (PR-3, doc 12 §5.1).
  - Protocol version bump, coordinated with N4.

### 5.2 Direct control and the order lapse

*Derived* from RK-10, D-016 and D-162.

- **A fleet is under direct control** from the moment its commander's own client, or its Strategist, gives it any fleet order. It stays under direct control until:
  - a `Release` command;
  - the D-162 lapse after the 120 s grace;
  - its destruction.

  Splits inherit direct control; ships built later join doctrine.
- **While under direct control,** the Deputy issues no command to the fleet (§4.3). Auto-targeting and rules of engagement still work (S5a). The view marks the fleet, so the HUD can show it (RK-10).
- **The retreat opt-in:** `Command::Fleet` gains a `retreat` flag. When set, the doctrine's retreat threshold applies during that order, and the Deputy retreats the fleet below the threshold, the one exception to the rule above (RK-10).
- **The lapse:** S6a's lapse after the grace now clears direct control, and the fleet falls back to its doctrine role (D-162). Until A1, it station-keeps (S6 plan §4.2).
- **Holdings have no direct control.** The Deputy and the player can both use abilities; cooldowns prevent double use. The Deputy changes the power profile only when an alert starts or ends, so a manual change holds until the next alert transition (*Proposed*).

### 5.3 Threat assessment and alert

*Proposed* in doc 13 §4 item 1 (Q207); details *Proposed*.

- **Every 0.5 s per holding:** the holding is on alert if either:
  - a hostile ship in the commander's visibility set is inside its defended zone (doctrine radius around the site); or
  - the inbox shows hostile fire on the holding, its modules or dome, or on any own ship inside its zone.
- **Hostile** means any other owner (S5a's `hostile`: no alliances in the MVP, D-026, D-040).
- **The alert ends** 60 s after the last such event (doc 13 §4).
- **Fire on an asset outside every zone** puts no holding on alert. That fleet's own doctrine still answers it: return fire (S5a), retreat (§5.5), and evacuation for convoys (§5.8).
- **Events:** `AlertStarted` and `AlertEnded` go to the commander's Deputy events, for the away report.

### 5.4 Alert actions

*Proposed* in doc 13 §4 item 2 (Q207); thresholds *Agreed* as starting values (D-082).

- **Alert start:** switch to the doctrine's alert power profile (`SetPowerProfile`). At alert end, switch back to the profile that was set before the alert.
- **Launch the garrison** of that holding: per fleet `Launch`, never the whole dock, so reserves and convoys stay docked (Q208).
- **Abilities** (S5b):
  - SHLD SURGE on a non-open sector below the doctrine threshold (Fortress 40%, Balanced 30%, Trader 20%), if power and the cooldown allow;
  - FLAK SCREEN when that many light hostiles are within 3 km of the dome centre (6 or 8; never for Trader).
- **Evacuation:** convoys near the holding (§5.8).
- **Alert end:** after 60 s the garrison docks again, and the profile returns.

### 5.5 Engagement

*Proposed* in doc 13 §4 item 3 (Q207).

- **Target priority** for Deputy-run armed fleets, as explicit `Attack` orders (D-144: explicit targets override auto-targeting):
  1. hostiles that damaged our structures in the last 30 s, from the inbox;
  2. the target the counter triangle favours: the highest multiplier of the fleet's main weapon against the target's size class (D-052);
  3. the nearest;
  4. the lowest `SimId`.

  A new `Attack` is issued only when the choice changes, to save command rate.
- **Rules of engagement in the zone:** garrison and patrol fleets inside the defended zone use the doctrine's zone rules (weapons free, or return fire for Trader). Turrets keep S5b's behaviour (fire unless STOP).
- **Leash:** a target farther from the fleet's anchor than the leash (8, 12 or 6 km) is dropped, and the fleet returns to its anchor. The anchor is the holding for a garrison, the nearest route point for a patrol, and the escorted fleet for an escort. This covers D-016's "anything that fires on us, up to the leash".
- **Retreat:** when the fleet's hull (sum over ships, as a share of maximum) falls below the threshold (50%, 40% or 30%), `Retreat` to the nearest own holding with room (S3).

### 5.6 Fleet roles

*Agreed* list (doc 12 §5.2: garrison, patrol route, escort, reserve); the behaviour is *Proposed* (Q208).

| Role | Idle | On alert at its holding | Notes |
|---|---|---|---|
| **Garrison** of a holding | Docked there; station-keeps by the core if docking is full (D-102) or the holding is an outpost | Launch, engage in zone up to the leash, dock 60 s after the alert ends | Default for new ships, at the holding that built them |
| **Reserve** at a holding | Docked; never launched by the Deputy | Stays docked | Default for MULEs not on a convoy, and for the start kit's 2 MULEs |
| **Patrol** a route | `Patrol` order on the route | Engages within the leash of the route | The route is a list of points |
| **Escort** a fleet | `Guard` order on that fleet | Engages attackers of the escortee within the leash | Released if the escortee is destroyed |

- The start kit's 3 VX are the mother station's garrison.
- **Repair at dock:** a fleet below full hull with no alert returns to dock and repairs there (D-071), unless its role is patrol and its hull is above the retreat threshold.

### 5.7 Rebuilding

*Agreed* authority (D-016: rebuild losses from an owner-approved list within a daily budget); the scope and budget window are *Proposed* (Q209).

- **The replacement list** holds entries `(fleet, class, count)`, for example "keep CAP WING at 3 × VX". When an own ship of a listed class is lost (inbox), the Deputy queues one replacement:
  - at the nearest own holding by route time that can build the class (frigates need a dry dock, D-068) and has queue room;
  - with the rally set to that fleet.
  
  If the fleet no longer exists, the replacement forms a new fleet with the entry's role.
- **Modules** (Q209): when a module is destroyed (inside the window, D-169, or in a siege), and the holding still belongs to the commander, the Deputy queues the same module in the same slot, if the holding doctrine's "rebuild modules" is on (the default).
- **The budget:** the preset's share (50%, 30% or 15%) of each resource's income over the last 24 h, against replacement spending over the same rolling 24 h. The Deputy samples `income_last_hour` hourly into a 24-slot ledger in its memory, so S4 needs no change.
- **Deferred decisions:** a replacement that does not fit the budget, the queue or any builder is not queued. The Deputy reports `Deferred { reason }` (budget exhausted, no builder, queue full) for the away report (doc 12 §5.3: "decisions the Deputy deferred"). It retries on the next budget change.
- **The Deputy never starts new projects** (D-016): it queues only listed replacements and rebuilt modules.

### 5.8 Convoys

*Agreed:* the Deputy runs convoy routes (D-016, D-064); a contested gate means reroute if possible, else hold at the last safe stop (doc 13 §4 item 5). Evacuation is *Proposed* (Q211).

- **S4 already moves convoys mechanically** (D-134). The Deputy adds safety:
  - **Evacuation:** when a sensed hostile is within the doctrine's evacuation radius (30, 20 or 10 km) of a convoy or of its next stop, the convoy docks at the nearest own holding on its route. Docked ships cannot be targeted. The Deputy re-issues the route 60 s after no hostile is within the radius. A command to a convoy fleet ends its route (S4), so the Deputy keeps the route in its memory.
  - **Contested gates** (D-055): when a convoy is held at a contested gate (S4/S6), the Deputy reroutes it if a route avoids that gate. Otherwise the convoy holds docked at its last stop. This needs a route query with gates to avoid, from S3's routing, through `View`.
- **Events:** `ConvoyHeld`, `ConvoyRerouted`, `ConvoyResumed`.

### 5.9 Whose Deputy

*Derived* from D-013, D-077 and D-079.

- **Every commander's Deputy is the same code.** For an AI commander, its Strategist writes the doctrine (A2 §4.8). Until A2, AI commanders keep Balanced defaults.
- **A human's Deputy** reacts after 0.3 s and issues at most 4 commands per second (Q202). An AI's Deputy follows its difficulty: 0.6 s and 2 per second at Normal.
- **During catch-up** every commander is offline (D-047), every Deputy defends, and no Strategist runs (D-080).

## 6. Timeline check

*Derived* from the agreed values; AI behaviour as proposed here.

**A raid on an offline AI holding, Balanced doctrine, Normal difficulty.** Holding H in N1 has 3 VX docked as its garrison. A convoy MULE is on its way to H. A raid of 4 raider strike craft (Light, signature 0.6) arrives through the gate 30 km from H. Times are rounded.

| Time | Event | Rule |
|---|---|---|
| t | Raiders leave the jump shroud 30 km from H and cruise towards it | D-055 |
| t + 7 s | Raiders come within 24 km of H (40 km × 0.6) and are detected at the next sensors pass | D-072, D-073, D-173 |
| t + 12 s | Raiders are within 20 km of H, the convoy's next stop: the Deputy sends the convoy to dock at H, applied after the 0.6 s latency | Q211, D-079 |
| t + 55 s | At tactical speed inside H's ops area, raiders cross the 10 km defended zone: H goes on alert at the next pass and Deputy slot (within 1 s) | §5.3 |
| t + 57 s | Shields first (alert profile), garrison `Launch`, then `Attack` on the nearest strike craft. At 2 commands per second the three take about 1.5 s | D-082, §4.3 |
| t + 75 s | VX (light cannon, 100% against Light) engage; one VX falls below 40% hull later and retreats to dock | D-052, §5.5 |
| t + 4 min | Raiders lose half their value and withdraw (A3); the VX stop at the 12 km leash | D-081, §5.5 |
| t + 5 min | The alert ends 60 s after the last event; the garrison docks and the profile returns; the convoy resumes 60 s after the last hostile left 20 km; the lost VX is queued as a replacement within the 30% budget | §5.3–§5.8, Q209 |

The owner never logged in. The away report later lists the alert, the losses on both sides, the replacement and the convoy hold.

## 7. Questions

Numbered Q200–Q212; A2 continues with Q213–Q219 and A3 with Q220–Q226 (Q227–Q229 unused). Each has a recommendation and alternatives.

- **✅ Q200 (D-199) — How AI code is kept from reaching `UniverseState`** (D-083 item 8). Inside one crate, Rust cannot stop a module from naming its ancestors' items, so S7's module-visibility check cannot work as written.
  *Recommendation:* a `nomad-ai` crate with all AI logic. It depends only on `nomad-types` and defines a `View` trait, which `nomad-sim` implements over `CommanderView`, and `nomad-sim` calls the AI from its phases. Cargo's ban on dependency cycles then guarantees the AI cannot name `UniverseState`. Amends D-086, as doc 11 §9 anticipated. *Alternatives:* AI modules inside `nomad-sim`, with a test that scans their source for forbidden paths (`UniverseState`, `crate::state`, `super::`; cheap but evadable); a `nomad-ai` crate that depends on `nomad-sim` and is limited to `CommanderView` by convention, with a lint or `cargo deny` rule (it can still name the state).
- **✅ Q201 (D-200) — How AI commands enter the step.**
  *Recommendation:* AI layers write to a per-owner outbox in the state with a `not_before` tick. Phase 1 of a later tick applies due AI commands after the tick's external inputs, in `(owner, layer, sequence)` order, through the same `execute`. Deputy commands to a fleet under direct control are dropped when applied (RK-10). *Alternatives:* apply AI commands in the same tick, right after the AI phase (breaks doc 11 §4.1's "apply on the next tick" and the latency rule); apply AI commands before external inputs (then a human's order and a Deputy order in one tick resolve in the Deputy's favour).
- **✅ Q202 (D-201) — The competence cap's mechanics** (doc 12 §5.4, doc 13 §4 item 6, D-079).
  *Recommendation:* latency 10, 6 or 3 ticks by difficulty, and 3 ticks for a human's Deputy, counted from the triggering event. At most the difficulty's command rate (1, 2 or 4 per second; 4 for a human's Deputy) in any 10 consecutive ticks, shared by all of a commander's AI layers. Waiting commands drain oldest first, Deputy before Strategist, and expire after 10 s. *Alternatives:* a token bucket with a one-second burst (simpler, but up to twice the rate inside one second); a human's Deputy at Normal values (0.6 s, 2 per second); separate rate budgets per layer.
- **✅ Q203 (D-202) — How the AI learns about events.**
  *Recommendation:* a per-owner inbox in the state, filled from each tick's outputs with only what the owner may know (own damage coalesced per entity and attacker, own losses, own fleet and economy events, addressed announcements, public season events, its AI commands' outcomes), kept for 60 s and read through `View`. *Alternatives:* the AI compares its own state between evaluations (no attacker identity, misses short events); the AI receives the previous tick's `TickOutputs` directly (not in the state, so a snapshot between two ticks loses them, and filtering by owner is easy to get wrong).
- **✅ Q204 (D-203) — Where AI evaluation runs, and its budget.**
  *Recommendation:* the Strategist in phase 3, staggered by commander over its interval; Deputies and raid-group control in phase 4 at 2 Hz, staggered by `SimId`; raid launches on a phase-2 timer. Budgets in a release build at stage 1: the Strategist's mean ≤ 5 ms per evaluation (D-083), bounded by deterministic work counters, never by time; Deputies and raid control together ≤ 25% of a 500× fast-forward tick (about 50 µs mean). The harness times `nomad-ai` directly and compares tick times with the AI on and off. *Alternatives:* run the Deputy at 10 Hz, event-driven (faster reactions for more cost); budget only live ticks (100 ms) and let catch-up be slower; a wall-clock time cap measured by the server (breaks determinism).
- **✅ Q205 (D-204) — AI presence before A5.**
  *Recommendation:* move D-080's seeded schedule generator into A2, running in the simulation from the seed (not journaled). It drives S6a's presence and the AI's initial window (D-157), so the Strategist knows when its session ends. `SetPresence` stays as a harness override. A5 keeps personalities, difficulty and schedule tuning. *Alternatives:* the harness sends `SetPresence` from its own seeded generator (the Strategist cannot see its session end, so trips that outlast a session lapse, D-162); always online at α (overstates the pace humans see); a journaled `SetSchedule` operator input at creation.
- **✅ Q206 (D-205) — The build order towards α.**
  *Recommendation:* S8 now, in parallel with S5b and S6a; S7 after S6a, in parallel with S6b; A1a (crate, outbox, gate, inbox, doctrine, direct control, rebuilding, convoy rerouting) after S6a; A1b (threat, alert, engagement, abilities, convoy evacuation) after S7 and S5b; A2 after A1a and S7; A3 after S6b, S7 and A1a, in parallel with A2. Amends D-095's serial order. *Alternatives:* D-095 as agreed (S6b → S7 → A1 → A2 → A3; longest path); A1 in one piece after S7 and S5b.
- **✅ Q207 (D-206) — Doc 13 §4 as the Deputy's rules.** Doc 13 §4 is still marked *Proposed*.
  *Recommendation:* adopt it, with the details in §5: alert on a hostile in the zone or fire on an asset in the zone, ending 60 s later; alert profile, garrison launch and ability triggers from the preset; target priority as explicit attacks; leash from the role's anchor; retreat by fleet hull; convoy safety. *Alternatives:* adopt it but without the alert power profile switch (the player's profile always stands); a fixed alert radius from the core sensor instead of the doctrine's zone.
- **✅ Q208 (D-207) — Fleet roles and the default role.**
  *Recommendation:* new ships join the garrison of the holding that built them (or its rally fleet). A garrison docks at its holding, launches on alert, engages in the zone up to the leash and docks 60 s after the alert. A reserve stays docked. Patrol and escort run as `Patrol` and `Guard` orders, with the leash measured from the route or the escortee. MULEs not on a convoy are reserves. *Alternatives:* new ships are reserves until a role is assigned (safer offline, idle by default); a garrison station-keeps outside the dome instead of docking (reacts faster, exposed to raids).
- **✅ Q209 (D-208) — What the replacement list covers, and its budget window.**
  *Recommendation:* listed ships and destroyed modules (rebuilt in the same slot, on by default per holding). Budget: the preset's share of each resource's income over the last 24 h, against replacement spending over the same rolling 24 h. Anything that does not fit is reported as deferred. *Alternatives:* ships only, as doc 13 §4 item 4 says (an offline player's lost solar array or siege-damaged modules stay lost until they log in); a budget that resets at 00:00 UTC.
- **✅ Q210 (D-209) — What "share of the fleet kept as garrison" means** (doc 13 §9).
  *Recommendation:* a Strategist parameter. The Strategist commits at most (1 − share) of its fleet value to plans away from its holdings, and when it writes doctrine before going offline every uncommitted fleet becomes a garrison. For a human it only sets the doctrine editor's default role for new fleets. *Alternatives:* the Deputy enforces it by recalling fleets above the share (this would override direct orders, against RK-10); purely advisory for everyone.
- **✅ Q211 (D-210) — Convoy evacuation and holding.**
  *Recommendation:* when a sensed hostile is within the evacuation radius of a convoy or its next stop, the convoy docks at the nearest own holding on its route and resumes 60 s after no hostile is within the radius. At a contested gate it reroutes if another route exists, otherwise it holds docked at its last stop. *Alternatives:* station-keep where it is instead of docking (stays targetable); ignore evacuation for convoys inside depot systems.
- **✅ Q212 (D-211) — Where AI parameters live.**
  *Recommendation:* an `ai` section in the rules file (D-104): presets, the difficulty table, the human Deputy's competence, inbox and outbox timings, Strategist hysteresis and noise, blueprints, raid parameters. They are copied into the Universe, so a season keeps its AI, replay stays exact, and A6 tunes them with `--rules`. *Alternatives:* constants compiled into `nomad-ai` (a rebuild per tuning step, and a new build changes running seasons); a separate AI file next to the rules file.

## 8. Tasks, in order

**A1a (after S6a; in parallel with S6b and S7, Q206)**

1. **The crate:** `nomad-ai` with the `View` trait, memory types and the manifest test (Q200); `nomad-sim` wiring for empty phases 3–4; the outbox and the competence gate (Q201, Q202); the inbox (Q203); per-layer counters. AI parameters in the rules file (Q212). Snapshot and rules format bumps; pins re-recorded.
2. **Doctrine:** types in `nomad-types`, `Command::Doctrine` and its validation (no doctrine commands from the Deputy layer), state, views, presets. Protocol version bump, coordinated with N4.
3. **Direct control** (RK-10): the per-fleet control state, `Release`, the `retreat` flag on fleet orders; S6a's lapse falls back to doctrine (D-162).
4. **Rebuilding** (Q209) with the 24 h ledger and deferred events. **Convoys** (Q211): rerouting around contested gates with a gate-avoiding route query, and holding. Evacuation needs sensed hostiles and lands with task 6.

**A1b (after S7 and S5b)**

5. **Threat and alert** through `View` and the inbox (§5.3).
6. **Alert actions:** profile, garrison launch, SHLD SURGE and FLAK SCREEN on their triggers (§5.4); convoy evacuation (§5.8).
7. **Engagement and roles:** priority, leash, retreat, the four roles, repair at dock (§5.5, §5.6, Q208).
8. **Harness `--deputy`** (§9) and the budget measurement (Q204). S5b's breach scenario re-measured with the Deputy's default ability use (D-147).
9. **Docs:** doc 12 §5 gains RK-10's addendum and notes for Q207–Q211; doc 13 §4 becomes agreed text; doc 20 §3 gains `nomad-ai`; `AGENTS.md`'s layout; doc 30's A1 row; this plan's status.

## 9. Done when

- **CI** is green on both operating systems, with `--locked`; `cargo deny` passes.
- **Determinism:** the same seed and script give the same hash with Deputies active. Snapshot and restore during an alert, with commands waiting in the outbox and inside a latency delay, change nothing.
- **Fairness:**
  - `nomad-ai`'s manifest test passes;
  - every Deputy command goes through the validator (per-layer counters reconcile);
  - an invalid command is rejected identically from a client and from the outbox;
  - `View` exposes no other commander's presence, kind or doctrine.
- **Competence:** a property test over random scenarios shows that no AI command applies before trigger + latency, and no commander's AI exceeds its rate in any 10 ticks.
- **`nomad-harness --deputy` scenarios**, each from scripted inputs:
  1. a raid on a holding: alert within one sensors pass, one Deputy slot and the latency of the zone crossing; profile switch; garrison launch; priority targeting; no pursuit beyond the leash; retreat below the threshold; docking 60 s after the alert;
  2. SHLD SURGE at the preset threshold, and FLAK SCREEN at the light-hostile count;
  3. convoy evacuation and resumption; rerouting around a contested gate, and holding when no other route exists;
  4. a replacement within the budget, and a deferral when over it; a destroyed module rebuilt;
  5. direct control: a human order is never overridden; retreat only with the opt-in; `Release` and the D-162 lapse return the fleet to its role;
  6. catch-up: Deputies defend while every commander is offline.
- **Budget:** at stage 1 in a release build, Deputy cost ≤ 25% of the tick at 500× fast-forward, reported as mean and p99; catch-up with Deputies keeps ≥ 500× (doc 11 §4.3).
- **Validator:** every new rejection reason is covered.

## 10. Stress test

1. **The `View` trait is all the AI can see** (S7 plan §8 item 2), and Q200 makes it a hard boundary. Every query A1–A4 need must exist on it, and every value type must be reachable from `nomad-ai`. That is more up-front design, and some churn while S7, A1 and N4 settle their types. In return, the fairness guarantee is real, and the AI can be unit-tested against a fake `View`.
2. **Direct control can leave units idle** (RK-10). A player who forgets to release a fleet leaves it outside doctrine until they disconnect. The HUD marker (RK-10) and the lapse limit the damage. A4's Strategist must release fleets itself when its plans end.
3. **The competence cap makes the Deputy slower than the alert.** At Normal, alert, launch and target selection take about 1.5 s of command rate after a 0.6 s latency. Against a raid that is fine. Against a fleet built to beat the Deputy it is deliberate (PR-5).
4. **Docking garrisons is safe but slow.** A docked garrison is untargetable and repairs, but launching costs a command and the 10 s shroud does not apply. A raid that strikes the solar array (outside the dome) gets a minute or so before the garrison engages. That is the intended peripheral pressure (doc 12 §3).
5. **Module rebuilding (Q209) spends while the player is away.** The budget caps it, and the away report lists it. Without it, an offline player loses a solar array inside their window and returns to a holding with no power.
6. **Evacuation trades income for safety.** A convoy that docks on every sighting delivers less. The Trader preset's 10 km radius is the honest lever, and the harness measures convoy uptime.
7. **Budget headroom is narrow in catch-up.** 50 µs per tick for every Deputy at stage 1 needs the calm-case shortcut (§4.5). At stage 2, with 10 commanders, the measurement decides whether Deputies must skip slots in quiet systems.

## 11. Conflicts and gaps found

1. **The S7 plan's fairness check cannot work as written** (§4.1). It is not an agreed decision; Q200 replaces it.
2. **D-078 against D-079:** D-078 re-evaluates the Strategist "every 10 s", but D-079's table gives 30, 15 and 10 s by difficulty, and Normal (the MVP default) is 15 s. *Derived reading:* 10 s is Hard's interval, and the table governs. Doc 11 §4.1's "every 10 s" reads the same way.
3. **Doc 13 is agreed, but its §4 (the Deputy's rules) is still marked *Proposed*.** Q207 asks for it.
4. **RK-10 is decided in doc 04, but doc 12 has no addendum.** A1 task 9 writes it.
5. **D-095 orders S7 after S6b,** but S7 does not depend on S6b (S7 plan header). Q206 proposes running them in parallel.
6. **D-162 and PR-3 bind expansion to presence.** That is consistent, but it means a Strategist without a known schedule cannot plan trips (§4.7, Q205).

## 12. As built in A1a

*Derived* unless marked. The owner approved A1a's *Proposed* readings as D-285 to D-288.

- **Versions:** snapshot format 17, rules `nomad-rules@11`, protocol 5 (new wire reason `NotPermitted` for `DoctrineByDeputy`).
- **Gaps closed:** D-055's contested-gate flag was never built by S4 or S6b; A1a added it (`gates.contested_radius_m`, 5 km, checked on demand in `economy/logistics.rs`). It counts every armed hostile; D-285 makes it count only detected ones, in A1b. `CombatEvent::ShipDestroyed` gained `fleet` and `EconomyEvent::ModuleDestroyed` gained `slot`.
- **Left for A1b:** the start kit as two fleets (D-286); fleet roles' behaviour, including a lapsed or released fleet returning to its role (it station-keeps until then); attacker fleets in combat events (the inbox names the attacker's owner only); D-285's switch to detection.
- **Left for A2 and A3:** the AI random streams (§4.6) are not created yet, since nothing draws from them; the inbox carries only the `OrdersLapsed` announcement, and other announcements and public season events arrive with the layer that needs them.
- **Readings (D-288), by file:**
  - `crates/nomad-types/src/doctrine.rs`: overrides are whole, `None` follows the preset; distances in whole metres, thresholds per mille.
  - `crates/nomad-sim/src/rules/ai.rs`: repair at dock on in every preset; at most 32 replacement entries.
  - `crates/nomad-sim/src/ai/mod.rs`: an NPC owner drains at Normal's values.
  - `crates/nomad-sim/src/ai/gate.rs`: a rejected command counts against the rate, a dropped one does not; expiry counts from when the command fell due.
  - `crates/nomad-ai/src/inbox.rs`: other announcements join when a layer needs them; a command outcome withholds the validator's reason (D-123).
  - `crates/nomad-sim/src/doctrine/mod.rs`: the retreat opt-in is the last direct order's flag; `CreateFleet` takes the control of the first listed ship's fleet, a merge keeps the receiving fleet's; a convoy route releases its fleet; stale overrides are pruned at the end of each tick.
  - `crates/nomad-ai/src/deputy/rebuild.rs`: "keep at count" counts replacements on their way; one ledger sample per clock hour (the first hour's budget is zero); spending counts at queueing and returns on refusal or drop; with the fleet gone, "nearest" is from the entry role's holding and the rally is cleared so the ship forms a new fleet; a retry's latency counts from the evaluation that retries it; unanswered waits for a new fleet are dropped after a day.
  - `crates/nomad-ai/src/deputy/convoy.rs`: a detour avoids only the gate the convoy is held for; each jump of a detour is checked to take the detour's lane; a held convoy is reconsidered every evaluation; a failed detour re-issues the route where the fleet is.
  - `crates/nomad-sim/src/economy/command.rs`: a route given to a fleet docked at one of its stops starts there (D-287).
  - `crates/nomad-sim/src/commander/presence.rs`: D-162's "direct orders" are the orders of fleets under direct control.
- **Cost:** in Debug, Deputies add about 4–5% to the mean tick on `--fleets --hours 1`. The release measurement is A1b task 8.

## 13. As built in A1b

*Derived* unless marked. The *Proposed* readings below wait for the owner's approval.

- **Versions:** snapshot format 18 (the attacking fleet in the inbox's damage entries; the Deputy's alerts, fleet homes, unanswered orders, ships lost since docking and evacuations in its memory), rules `nomad-rules@12` (the `ai.deputy` timings and distances), protocol 5 (unchanged: no wire type changed).
- **Decisions built:** D-285 (a gate is contested for a convoy only by hostiles its owner detects, `economy/logistics.rs`), D-286 (the start kit as two fleets, `fleet/mod.rs`; pins re-recorded), and A1a's gap 5: `CombatEvent::ShipDamaged`, `DefenceEvent::SectorDamaged` and `ModuleDamaged` carry `by_fleet`, and the inbox keeps it only while the damaged asset's owner detects that fleet.
- **`View`** gained sensed fleets (over `CommanderView::sensed_fleets_in`), own defence (sector strengths, dome, ability readiness), the power profile, fleet hull, order, rules of engagement and engagement, docking room, a validator check (`accepts`), and class data (armed, light, hull, the D-052 multiplier).
- **Where it lives:** `nomad-ai/src/deputy/threat.rs` (alert and alert actions), `fleets.rs` (roles, engagement, retreat, repair, default roles), `convoy.rs` (evacuation before the contested-gate hold and detour); `TickOutputs::deputy_events` carries `AlertStarted`, `AlertEnded`, `ConvoyEvacuated` and `ConvoyResumed` with A1a's events.
- **Harness:** `nomad-harness --deputy` runs §9's six scenarios with their checks (all pass on seeds 0, 1, 2, 7 and 99); `--deputy bench` measures the budget. `--combat breach` now runs the defender's Deputy (D-147).
- **Measured** (release build, stage 1, catch-up, a raid every 30 min, 6 simulated hours, seed 1; seed 7 alike): every Deputy together costs 12.1 µs a tick on average (6.1% of a 500× tick of 200 µs) and 43 µs at p99 (21.6%), within D-203's 25%; catch-up runs at about 1,900× real time (≥ 500×). The breach scenario with the Deputy's default ability use takes 7.5 min (three SHLD SURGEs), inside D-058's 5–10 min; without abilities 4.7 min.
- **Readings (*Proposed*), by file:**
  - `crates/nomad-ai/src/deputy/threat.rs`: a sensed hostile's event is the sensors pass that saw it, so the latency counts from the pass; fire on an own ship counts for a holding when its fleet is in the zone as the entry is read; the profile changes only at alert start (if it differs) and end (back to the remembered one, if it differs now); SHLD SURGE takes the closed sector with the lowest share; FLAK SCREEN counts every sensed light hostile, shrouded or not; an ability is not issued again while the last is unanswered.
  - `crates/nomad-ai/src/deputy/fleets.rs`: a fleet's hull for the retreat threshold counts, in its maximum, the ships it lost since it last docked (otherwise focused fire, one ship at a time, never brings a retreat); a retreating fleet is left to its retreat; a fleet's home (for its default role) is where a ship that formed it was built, else where it first launched from or was seen docked, and a fleet with none (formed in space) is left alone; a garrison launches on alert only at or above its retreat threshold, a docked patrol or escort only once repaired; targets are sensed unshrouded ships within the leash of the anchor, and the current target is kept while it ranks level with the best on priority and multiplier; the main weapon is that of the most numerous armed class; an escort's "no alert" is "not engaged"; nothing new is issued to a fleet while its last command is unanswered; "by the core" is within 1 km of the launch point (rules `ai.deputy.anchor_radius_m`).
  - `crates/nomad-ai/src/deputy/convoy.rs`: the evacuation radius is the next stop's holding doctrine, measured from the stop's site (or that of the holding the convoy is docked at); any sensed ship of another owner counts, armed or not; a convoy docked at a stop ends its route there, one in space retreats to the stop of its route nearest by route time, one in lane transit is weighed on arrival; evacuation keeps the route of a hold or detour it takes over; the latency counts from the sensors pass.
  - `crates/nomad-ai/src/inbox.rs`: damage is summed per entity, attacking owner and attacking fleet; hits from undetected fleets, turrets and FLAK SCREEN sum into one entry without a fleet.
  - `crates/nomad-harness/src/station.rs`: in D-147's breach measurement the start kit's fleets are reserves, since D-058's scenario is a fleet against a station.
- **Gaps found:** doc 13 §4 item 2 and §9 name "harvesting craft", but the MVP has none (harvesters are modules, doc 14 §6), so evacuation covers convoys only. A fleet whose escortee is gone falls back to its default role rather than being released by the Deputy (doc 13 §4: "released"), since a role change is doctrine and the Deputy may not write doctrine (PR-3).
- **Left for A2–A4:** the Strategist writes doctrine for AI commanders (until A2 they keep Balanced) and must release its fleets when its plans end (§10 item 2); A3's raid groups show up as sensed hostiles, so the Deputy needs nothing new for them; `View` already carries what A2's defence planning reads (sensed fleets, defence, hull). The AI random streams are still not created (nothing draws from them).
