# Work package S7 — Sensors and fog of war

Status: **Planned and decided** (Q166–Q174 → D-173–D-181). Written together with `N4-replication.md`, because N4 sends each client exactly what S7 decides its commander can see. Builds on S3 (ships, fleets, holdings, lane transit, the validator), S5a (targeting, the engagement trigger) and S4 (modules, the economy views). This plan is also the brief for whoever builds S7. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Goal

Every commander, and every NPC faction, knows only what its sensors detect, what is public, what it owns and what it remembers. The simulation computes a visibility set per observer in the 2 Hz sensors phase, keeps last-known memory and lane traffic per commander, and offers one read-only projection, the **`CommanderView`**, through which the AI, the Deputy, the raider director and N4's replication all see the Universe (D-077, doc 16 §7). S3's and S5a's "everything is visible" stand-in is replaced by real visibility in the validator, in target acquisition and in the cruise and engagement triggers. All of it is deterministic, part of the snapshot, and cheap enough at stage-2 scale.

## 2. Inputs

- **Information model:** doc 16 in full. Agreed: §2 information classes (D-075), §3 detection and sensors (D-072, D-073), §4 fleets in transit (D-074), §5 last-known memory (D-076), §7 `CommanderView` (D-077). *Derived:* §6 replication (N4).
- **Combat:** doc 15 §3.3 (cruise mode drops at 20 km), §4 (lock builds only while detected); D-114 (an engagement ends after 30 s with no hostile detected within 20 km and no fire taken); the S5 plan §4.4 (engagement) and §4.7 (every MVP weapon range lies inside its own platform's detection range, so S7 cannot change who can be fired on).
- **AI:** doc 13 §2–§5 (the AI reads only `CommanderView` and acts only through commands, D-027, D-077), §8 (raiders see through their faction's sensors), §10 and D-083 (Strategist evaluation ≤ 5 ms mean per commander; within the tick budget).
- **Offline rules:** doc 12 §4 and D-075 (siege announcements go to the owner and the commanders with breach credit, D-057; produced by S6, read through `CommanderView`).
- **Economy:** the S4 plan §4.8 (read-only economy views per commander, which S7 wraps into `CommanderView`).
- **Simulation:** doc 11 §4.1 (sensors phase 6 at 2 Hz), §4.2 (work per system in `SystemId` order), §4.3 (budget), §5 and D-049 (determinism), D-099 item 9 (format changes without migration before β).
- **Wire boundary:** D-123 (rejection reasons collapse "unknown", "not yours" and "not visible" into `NotFound`).
- **Code:** `crates/nomad-sim`: `Entities` on `hecs`, `Fleet`/`Ship`/`Motion`/`Shroud`, `FleetView`/`ShipView`, holdings, lane transits, `TickOutputs`, the validator in `command.rs` (whose module docs say every entity counts as visible), `set_fleet_engaged`; `Owner` and `FactionId` in `nomad-types`.
- **Work in flight:** S4 (economy), S5a (ship combat) and N3 (transport) are being built now. This plan relies on their plans, not on their exact names.

## 3. Scope

**In scope:**

- **Sensors in the rules file:** ranges per sensor kind and signatures per size class (D-072, D-073), plus the S5 rule check (weapon range < own sensor range × 0.6) as a rules-file check.
- **The sensors phase:** visibility sets per observer (commanders and NPC factions), at 2 Hz (Q166), including system-control coverage.
- **What carries sensors and what is detected:** ships, holdings and their modules, partly detected fleets (Q167–Q169).
- **Last-known memory** per commander: what is stored (Q170), expiry (D-076) and clearing (Q171).
- **Lane traffic:** observed departures (D-074, Q172).
- **`CommanderView`:** the read-only projection for the AI, the Deputy, the raider director and N4, with per-system indices for the AI budget.
- **Switching from "everything is visible":** the validator (a new `NotVisible` reason), target acquisition, lock, the cruise drop and D-114's engagement trigger; orders on targets that drop out of detection (Q173).
- **NPC observers** (Q174).
- **A build-time fairness check** (doc 16 §7): AI and Deputy code cannot reach `UniverseState`.
- **Harness:** a sensors cost report and a fog-of-war scenario.

**Out of scope:**

- **Replication, interest management and the wire format** of the view (N4).
- **Generating announcements and notifications** (S6 for sieges; S5 events for raids). S7 only makes them part of `CommanderView`.
- **Stealth, ECM and decoys** (none in the MVP, D-072).
- **AI behaviour** that uses the view: scouting, pickets, reacting to lane traffic (A2–A4).
- **The away report's history** (U5).

## 4. Design

### 4.1 Sensors and signatures

*Agreed* values (D-072, D-073); storage *Derived* from D-104.

- **The rules file gains a `sensors` section:** range at signature 1.0 per sensor kind (holding core 40 km; VX 15, LANCER 10, WARDEN 25, MULE 8; raider strike craft 12, HSTL 30), and the signature per size class (Light 0.6, Medium 1.0, Heavy 1.5, Structure 1.5). Ship classes name their sensor kind; the rules format version increases.
- **Rule check** (S5 plan §4.1, now real): for every armed platform, every weapon's range is below its own sensor range × the smallest signature. A rules file that breaks it is rejected, so "in range" always implies "detected by the shooter itself".
- **Detection** (D-072): target T is detected by observer O when some sensor S of O lies in T's system with `distance²(S, T) ≤ (range(S) × signature(T))²`. The comparison uses squared distances, so no `sqrt` is needed and the result is bit-exact on every platform (D-049).

### 4.2 Who senses and what is sensed

*Derived* from doc 16 §3 and D-073; the details are *Proposed* (Q167–Q169).

- **Observers** are owners: each commander and each NPC faction (`Owner`, doc 16 §7). The set is fixed per season, in `Owner` order.
- **Sensors** (Q169): the core of each completed holding (40 km), each ship in space by class (including ships under the arrival shroud, which may not fire but can see), and **system control**: a commander who controls a system detects everything in it, including its gate approaches (D-041, D-073). Docked ships add nothing (the core covers far more), fleets in lane transit sense nothing (doc 16 §4), and an outpost under construction has no core yet.
- **Targets:**
  - **Ships**, each with its class's signature (Q167). A fleet appears in a view when at least one of its ships is detected.
  - **Holdings** as one target at the dome centre with signature 1.5; detecting it reveals its modules, their kinds, hull and state (Q168). Every module lies within about 1.3 km of the centre, far below any sensor range at signature 1.5, so per-module detection would change almost nothing at many times the cost. A holding's existence, site and owner are public anyway (D-044, D-075).
  - **Not targets:** projectiles (N4 replicates their effects for sensed shooters and targets), lanes, gates and sites (public).
- **One shared picture** (doc 16 §3): whatever any of an observer's sensors detects, all its units can see and target.

### 4.3 The sensors phase

*Agreed* rate (doc 11 §4.1, doc 16 §3); the schedule is *Proposed* (Q166).

- **When:** on every tick divisible by 5 (2 Hz), for all observers at once (Q166). Everything else reads the sets from the last pass, so the view is consistent within each half second, and the same for every reader.
- **How:** per system in `SystemId` order: collect the sensors present, grouped by observer, and the targets; for each target in `SimId` order and each observer in `Owner` order, stop at the first sensor in range. System control marks every target in the system as detected for the controller.
- **Result per observer:** a `VisibilitySet` of detected ship and holding `SimId`s, indexed per system (a sorted vector per system). The per-system index is what keeps `CommanderView` queries cheap (§4.7).
- **Transitions:** comparing the new set with the previous one gives the entities that came into and went out of view. They drive last-known memory (§4.4) and visibility events in `TickOutputs` (for N4 and the harness).
- **Cost** (*Derived*, measured in task 9): with brute force per system, a pass costs about `sensors × targets` distance checks per system. Stage 1 (4 commanders and raiders, about 200 ships and 20 holdings over 9 systems) is a few thousand checks; stage 2 (10 commanders, about 600 ships and 60 holdings over 18 systems, all fighting in one system in the worst case) is at most about 100,000, well under 1 ms in a release build. No spatial grid is needed at MVP scale. Doc 16 §9 item 5's grid and staggering remain the path for the full 100-commander Universe.
- **Snapshots:** the visibility sets of the last pass are part of the state, because commands and triggers between passes depend on them and a restored state must not recompute them from later positions.

### 4.4 Last-known memory

*Agreed* rules (D-076); granularity and clearing are *Proposed* (Q170, Q171).

- **Per commander, in the state** (and so in snapshots, D-048).
- **Entries** (Q170):
  - a **fleet entry**: fleet `SimId`, owner, system, position, heading and velocity, composition (ship count per class), hull % per class, and the tick seen;
  - a **holding entry**: holding `SimId`, the observed modules (kind, slot, hull %, disabled), the dome sectors' strength if S5b exposes it, and the tick seen.
- **Written on loss of contact:** while an entity is sensed, its live state *is* its last observation, so memory is written only when it leaves the visibility set, from the state at the last pass where it was sensed. That keeps the cost at transitions, not at every pass. An entity that is sensed again replaces its entry.
- **Expiry** (D-076): fleet entries are dropped 24 h after the tick seen, swept once per minute per commander (staggered by commander). Holding entries stay until the holding is sensed again, or until public ownership shows it changed hands.
- **Clearing** (Q171): a fleet entry is also dropped when the commander's current picture covers its remembered position for its largest remembered signature, and the fleet is not detected.
- **NPC factions** keep no memory (Q174).

### 4.5 Lane traffic

*Agreed* model (D-074); the exact rule is *Proposed* (Q172).

- **A departure is observed** by every observer whose visibility set from the last pass contains a ship of the departing fleet when the fleet enters transit (Q172). A fleet spools up for 5 s within 2 km of its gate, so the last pass always falls inside that window.
- **The record:** fleet `SimId`, owner, composition as seen, origin system and gate, destination system, arrival tick. It is stored per commander and expires when the arrival is observed (the fleet is detected at the destination) or 10 min after the arrival tick, whichever comes first.
- **Own fleets in transit** are own information: the owner always knows them, with their arrival tick.
- **Arrivals** need no special rule: a fleet that arrives is detected normally at the destination gate approach, under its shroud (D-055).

### 4.6 Replacing "everything is visible"

*Derived* from doc 15 §2, §4 and doc 16 §3; Q173 is *Proposed*.

- **Validator** (D-077): orders that name another owner's entity (Attack, Attack-move onto an entity, Guard on a non-own object) require it to be in the issuer's current visibility set. If not, the new reason `NotVisible` applies. N3's wire mapping (D-123) collapses it into `NotFound`, together with "unknown" and "not yours", so a client cannot tell an unseen entity from a non-existent one. The exhaustive mapping test fails to compile until the mapping covers the new reason. Own entities and public objects (holdings, gates, sites) are always nameable.
- **Last-known entities are not targets:** a player who right-clicks a remembered fleet gets a Move to its remembered position from the client (C8, U2), not an Attack.
- **Target acquisition** (S5a): auto-targeting considers only hostiles in the commander's visibility set; lock builds only while the target stays in it (doc 15 §4). Because of the rule check in §4.1 this never changes a valid in-range target, only targets beyond the shooter's own sensors, which no MVP weapon can reach.
- **Cruise and engagement** (doc 15 §3.3, D-114): a fleet becomes engaged when a hostile in its commander's visibility set is within 20 km of any of its ships, or when it takes fire. The 30 s deadline from S5a is unchanged.
- **An Attack target that drops out of detection** (Q173): the fleet continues to the target's last-known position, then holds there, and the order completes with a `TargetLost` fleet event if the target is not detected again on the way.
- **Effect on TTK:** fleets will now find each other later. S5a's D-058 measurements are re-run after S7 (S5 plan §9 item 5).

### 4.7 `CommanderView`

*Agreed* principle (D-077, doc 16 §7); the API is *Proposed*.

- **A borrowed, read-only projection:** `CommanderView<'a>` holds `&'a UniverseState` and an observer. It is created on demand inside the step for the AI and the Deputy, and after the step for N4 (on the server's simulation thread). Nothing is copied unless a caller asks for it.
- **Its queries follow the information classes of doc 16 §2:**
  - **public:** topology, holding ownership, system control, standings (S8), exposure windows (S6);
  - **own:** own fleets, ships, holdings and modules in full; the S4 economy views (pool, income, command cap, queues, power, buffers); own fleets in transit;
  - **sensed:** per system, the detected fleets (with their detected ships only) and holdings (with modules), with position, heading, velocity, class, owner and hull %, and nothing private (no orders, cargo, doctrine or queues of others);
  - **last-known:** memory entries with their tick seen;
  - **lane traffic:** observed departures;
  - **announcements and notifications** addressed to this commander (S5/S6 events).
- **Indices for the AI budget** (D-083): every query that lists other owners' entities goes through the per-system index of the visibility set, so a Strategist evaluation touches only the systems it asks about.
- **NPC factions** get the same view, without memory, private economy or announcements (Q174).
- **Fairness, enforced at build time** (doc 16 §7): AI and Deputy modules receive only `CommanderView`, and a test in `nomad-sim` fails if their module tree can name `UniverseState` (module visibility plus a compile-fail test). The view's types expose no `&UniverseState` and no handle that leads back to it.

### 4.8 Determinism, performance and snapshots

*Derived* from doc 11 §4–§5.

- **Order:** systems in `SystemId` order, targets in `SimId` order, observers in `Owner` order; visibility sets and memory in sorted vectors and `BTreeMap`s; integer ticks; squared-distance comparisons in `f64` without transcendental functions.
- **Budget:** the sensors phase, memory transitions and the minute sweep together stay under 5% of the tick budget at stage 2 in the stress battle. The harness reports the phase's mean and p99 separately.
- **Snapshots:** one format bump for the visibility sets, memory and lane traffic. Hashes change as fleets now behave differently (the cruise drop and acquisition), which is allowed before β (D-099 item 9). Pins are re-recorded.

## 5. Questions

Numbered Q166–Q174; each has a recommendation and alternatives. N4's questions follow in `N4-replication.md`.

- **✅ Q166 (D-173) — When the sensors phase runs.** Doc 11 and doc 16 say 2 Hz, "staggered".
  *Recommendation:* all observers at once on every fifth tick, so each half second has one consistent picture; stagger only if the harness shows a spike. At MVP scale a pass costs well under 1 ms. *Alternatives:* staggered per observer across the five ticks (smoother load, but observers see the Universe at different ticks); every tick (10 Hz, five times the cost for no gameplay gain).
- **✅ Q167 (D-174) — Detecting part of a fleet.** Doc 16 detects targets, and targets are ships with different signatures. A fleet can mix a WARDEN (Heavy, 1.5) with LANCERs (Light, 0.6).
  *Recommendation:* detection per ship, as doc 16 says. A fleet appears when any member is detected, and lists only the members that are detected; its composition is partly known until all are in range. *Alternatives:* detecting any member reveals the whole fleet (simpler, but a cruiser then exposes its escorts at 2.5× their range); one test per fleet at its centre with its largest signature (cheapest, same effect as the previous one).
- **✅ Q168 (D-175) — Detecting holdings and modules.** A holding's site and owner are public, but its modules are not.
  *Recommendation:* the holding is one target at its dome centre with signature 1.5 (Structure); detecting it reveals all its modules, their hull and state. *Alternatives:* each module is a separate target (about 10× the targets for almost no difference, since all modules lie within 1.3 km of the centre); modules are public, like ownership (removes the scouting value of seeing a station's defences).
- **✅ Q169 (D-176) — What carries sensors.** Doc 16 lists holding cores and ship classes.
  *Recommendation:* completed holding cores and ships in space, including ships under the arrival shroud, plus system control. Not docked ships (the core already covers more), not fleets in transit (doc 16 §4), not outposts under construction, not modules. *Alternatives:* outposts under construction get a reduced sensor (for example 20 km) from the moment the kit lands; shrouded arrivals are blind for the 10 s shroud.
- **✅ Q170 (D-177) — What last-known memory stores.** D-076 says "the last observation of every entity". Per-ship and per-module entries multiply memory and snapshot size by the fleet and station sizes.
  *Recommendation:* one entry per fleet (composition by class, hull % per class, position, heading, velocity, tick seen) and one per holding (observed modules with kind, slot, hull % and state, tick seen). *Alternatives:* one entry per ship and per module; fleet entries only, with holdings known only from public data and live sensing.
- **✅ Q171 (D-178) — Clearing stale fleet entries.** D-076 keeps a fleet entry for 24 h. A commander who looks at the remembered spot and finds nothing would still see the ghost.
  *Recommendation:* drop a fleet entry as soon as the commander's current picture covers its remembered position (for its largest remembered signature) and the fleet is not detected. The 24 h expiry still applies otherwise. *Alternatives:* keep D-076's 24 h only; keep the entry but mark it "not at last position".
- **✅ Q172 (D-179) — When a departure is observed.** D-074 says anyone "who currently has sensor coverage of the origin gate" sees a departure. Coverage of a point and detection of a target differ when signatures differ.
  *Recommendation:* an observer sees the departure if a ship of the departing fleet was in its visibility set at the last pass before the fleet entered transit (always within the 5 s spool-up). The record holds the fleet's identity and composition as seen, destination and arrival tick, and expires when the arrival is observed or 10 min after the arrival tick. *Alternatives:* coverage means the gate point lies within a sensor's range at signature 1.0, whatever the fleet; the record holds only destination and arrival time, not the fleet's identity.
- **✅ Q173 (D-180) — An Attack target that drops out of detection.** Doc 15 does not say what happens.
  *Recommendation:* the fleet continues to the target's last-known position and holds there; the order completes with a `TargetLost` event if the target is not detected again on the way. *Alternatives:* the order is cancelled at once and the fleet holds where it is; the fleet keeps pursuing the true position (rejected in principle: that is information the commander does not have).
- **✅ Q174 (D-181) — NPC raiders and memory.** Doc 16 §7 says raiders see through their faction's sensors under the same rules; D-076 gives memory to commanders.
  *Recommendation:* NPC factions have visibility sets and lane traffic, but no last-known memory; the raider director (A3) works from public data and live sensing. *Alternatives:* NPC factions keep the same memory as commanders (more state, for raids that remember convoy routes); NPC factions see their own systems in full (simpler, but breaks "same rules").

## 6. Tasks, in order

1. **Rules:** the sensors section, signatures per size class, sensor kind per ship class, the range-versus-detection rule check. Rules format bump.
2. **Visibility sets:** observers, sensors (Q169), targets (Q167, Q168), system-control coverage, the sensors phase on the Q166 schedule, per-system indices, transition lists, visibility events in `TickOutputs`.
3. **Last-known memory:** entries (Q170), writing on loss of contact, the minute sweep with 24 h expiry, the ownership rule for holdings, clearing (Q171). Snapshot format bump.
4. **Lane traffic:** departure observation (Q172), records, expiry; own transits.
5. **`CommanderView`:** the projection and its queries by information class, including the S4 economy views and announcements; NPC views (Q174).
6. **Fairness check:** module boundaries so AI and Deputy code cannot name `UniverseState`, with a test that fails if they can.
7. **Switch-over:** the validator's `NotVisible` reason and its coverage; auto-targeting and lock; the cruise drop and D-114 trigger through visibility; the lost-target rule (Q173). Update N3's wire mapping if N3 has landed.
8. **Harness `--fog`:** a two-commander scenario that checks the picture against brute-force ground truth every pass, and re-runs S5a's TTK scenarios with real detection.
9. **Performance:** sensors phase mean and p99 at stage 1 and in a stage-2 stress battle (600 ships, all in one system), release build.

## 7. Done when

- **CI** is green on both operating systems, with `--locked`; `cargo deny` passes.
- **Determinism:** same seed and script give the same hash; snapshot and restore between two sensor passes, and with memory and lane traffic present, change nothing.
- **Detection tests:**
  - detection follows range × signature exactly at the boundary (squared distances);
  - system control reveals the whole system, gate approaches included;
  - a mixed fleet is detected member by member (Q167);
  - a holding reveals its modules when detected (Q168);
  - docked ships and fleets in transit sense nothing (Q169);
  - a departure is observed exactly when a member was visible at the last pass (Q172), and its record expires as agreed.
- **Memory tests:** an entity that leaves view leaves an entry with the last observed values; fleet entries expire after 24 h and on clearing (Q171); holding entries survive until re-observed or ownership changes; memory survives a snapshot round trip.
- **Switch-over tests:** an order naming an unseen entity is rejected with `NotVisible`, which reaches the wire as `NotFound`; auto-targeting ignores undetected hostiles; the cruise drop and D-114 fire only on detected hostiles; a lost Attack target ends at its last-known position (Q173).
- **Fairness:** the build-time check fails if AI or Deputy code can reach `UniverseState`.
- **`--fog`:** the picture matches brute-force ground truth over a simulated day; re-measured TTK is reported next to S5a's.
- **Performance:** at stage 2 the sensors phase stays under 5% of the tick budget (doc 11 §4.3) in a release build; mean and p99 are reported.

## 8. Stress test

1. **Fleets behave differently after S7.** Engagements start later and pursuits end at last-known positions, so S5a's TTK numbers and A2–A4's behaviour must be measured after S7, not before. That is planned (task 8), but it means α's TTK tuning cannot finish before S7.
2. **`CommanderView` is the AI's whole world.** If its queries are awkward or slow, A1–A4 pay for it in every evaluation (D-083's 5 ms). The per-system indices and query-by-class shape are meant to prevent that; A1 will show whether they do.
3. **Memory granularity is a trade-off** (Q170): fleet entries are cheap and match how strategic views show fleets, but they lose per-ship hull detail, which the SYSTEM view does not show anyway.
4. **Ghost clearing** (Q171) goes beyond D-076's letter. Without it, a commander sees a 24 h ghost at a spot they are looking at, which reads as a bug.
5. **A half-second picture** means an entity can stay "sensed" up to 0.5 s after it left range, or appear up to 0.5 s late. It is the same for every commander and the AI, so it is fair; the alternative (10 Hz) costs five times as much.
6. **Visibility sets in snapshots** grow the snapshot with observers × visible entities. At stage 2 that is small (tens of KB), but it must be measured with the rest of the snapshot.
7. **The "never reveal" property is easy to break** when new fields are added to views. The `CommanderView` types are the only door, the `--fog` ground-truth check catches leaks in the sim, and N4's oracle test catches them on the wire.

## 9. Effect on other documents

- **Doc 16:** none if the recommendations are adopted; §3, §4 and §5 gain notes for Q167–Q172.
- **Doc 15:** §3.3's cruise drop and D-114 read detection from the visibility set; §4 lock unchanged.
- **Doc 13:** the AI reads the `CommanderView` API described in §4.7.
- **Doc 20:** §4's "visibility sets" become the input to N4's per-client view extraction (`N4-replication.md`).
- **Doc 30:** S7 ✅ when done; S5's TTK measurement is repeated after S7.
