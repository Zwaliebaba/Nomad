# 02 — Open questions

Status: **All questions answered (Q1–Q138) on 2026-11-06.**

## Answer summary

| Q | Answer | Decision |
|---|---|---|
| Q1 | Continuous real time, about 10 Hz tick, layered timers | D-019 |
| Q2 | Seasons of 8–12 weeks | D-020 |
| Q3 | One Universe, up to 100 commanders | D-021 |
| Q4 | One galaxy; Sector, System and Holding are gameplay layers; gates and lanes only; one ops grid per holding; Kestrel-7 is a system | D-022 |
| Q5 | PvP plus NPC raiders | D-014 |
| Q6 | Doc 12 §5.1 authority table | D-016 |
| Q7 | Stakes rule (doc 12) | D-015 |
| Q8 | Yes: the Deputy is always on | D-013, D-016 |
| Q9 | Derelict after 14 days | D-023 |
| Q10 | A fallen mother station emergency-jumps; no elimination | D-017 |
| Q11 | The Universe keeps running (headless server) | D-026 |
| Q12 | Economy and building, fleets and combat, travel between systems; no diplomacy. Season won by system control | D-026, D-020 |
| Q13 | Local server first, containerisable; Windows client, Linux server | D-029 |
| Q14 | Phase 1 first, design in parallel | D-028 |
| Q15 | Fair AI commanders, free-for-all, simulated schedules | D-027 |
| Q16 | *World* is fine as a word; the total game space is the Universe | D-008 |
| Q17 | `hecs`, `tokio` + `quinn`, SQLite then PostgreSQL | D-030 |
| Q18 | Nomad is the working title | D-018 |
| Q19 | One daily 4 h window per commander, 48 h change delay | D-015 |
| Q20 | 12 h notice, 60 min siege, 48 h immunity | D-015 |
| Q21 | A fallen holding is captured | D-017 |
| Q22 | The Deputy is always on | D-013 |
| Q23 | Limited vacation mode | D-024 |
| Q24 | Storage caps | D-025 |
| Q25 | Proprietary, all rights reserved | D-031 |
| Q26 | Siege starts staggered 60 min apart, at most 4 per window | D-033 |
| Q27 | Formal alliances after the MVP, size-capped | D-040 |
| Q28 | Activity-derived exposure window | D-032 |
| Q29 | No time compression; human playtests in real time | D-036 |
| Q30 | Headless `nomad-sim` track in parallel with phase 1 | D-037 |
| Q31 | MVP stage 1: 3 AI commanders plus raiders in one sector; no vacation or derelict states | D-038 |
| Q32 | Sensor-based fog of war | D-039 |
| Q33 | Modules destroyed, nearest-holding jump, 72 h protection, comeback grant | D-034 |
| Q34 | Blueprints, doctrine presets and cosmetics carry over | D-035 |
| Q35 | System control by strict majority of completed holdings | D-041 |
| Q36 | Jump transit cannot be interrupted; interception in-system | D-042 |
| Q37 | Doc 10 scale values as starting point | D-043 |
| Q38 | New seeded, fully charted galaxy each season | D-044 |
| Q39 | Frontier sectors for late joiners | D-045 |
| Q40 | Shield arcs centred on pylons; grid north 225° | D-046 |
| Q41 | Catch up after downtime; operators may freeze | D-047 |
| Q42 | Snapshots plus input journal | D-048 |
| Q43 | Bit-identical determinism per build | D-049 |
| Q44 | Pure `nomad-sim` crate | D-050 |
| Q45 | Start building now: phase 1 plus the simulation core | D-051 |
| Q46 | Projectiles with lock, counter triangle | D-052 |
| Q47 | One-way, power-fed dome; minimal dome per core | D-053 |
| Q48 | Core kit, 2 h build, outposts can be destroyed | D-054 |
| Q49 | Gate shroud, spool-up, contested gates | D-055 |
| Q50 | Raiders never breach | D-056 |
| Q51 | Siege and capture details as written | D-057 |
| Q52 | Time-to-kill targets as tuning goal | D-058 |
| Q53 | Grid fade matches the reference render | D-059 |
| Q54 | Pivot keeps the projection formula | D-060 |
| Q55 | About 200 stars (spec) | D-061 |
| Q56 | Views named by facing: N, W, S, E | D-062 |
| Q57 | M1 defaults accepted; tuning values to config | D-063 |
| Q58 | Pooled stockpile fed by convoys | D-064 |
| Q59 | No munitions in the MVP | D-065 |
| Q60 | Never-ending, rate-limited deposits | D-066 |
| Q61 | Fixed build slots | D-067 |
| Q62 | One queue per holding; pay at start | D-068 |
| Q63 | No economic bonus for control | D-069 |
| Q64 | Economy pacing targets as tuning goal | D-070 |
| Q65 | Free slow repair plus paid REPAIR; no salvage, no upkeep | D-071 |
| Q66 | Binary detection with signatures | D-072 |
| Q67 | Sensor ranges; shared picture per commander | D-073 |
| Q68 | Departures seen at the origin gate | D-074 |
| Q69 | Windows public, online status private | D-075 |
| Q70 | Server-side last-known memory with expiry | D-076 |
| Q71 | Three-layer AI stack | D-077 |
| Q72 | Utility-goal Strategist | D-078 |
| Q73 | Difficulty changes competence only | D-079 |
| Q74 | Seeded AI online schedules | D-080 |
| Q75 | Raider director rules | D-081 |
| Q76 | Doctrine preset values as starting point | D-082 |
| Q77 | AI acceptance criteria | D-083 |
| Q78 | wgpu chooses the backend automatically | D-084 |
| Q79 | Client starts the headless server | D-085 |
| Q80 | Add `nomad-types`, `nomad-protocol`, `nomad-server` | D-086 |
| Q81 | QUIC with reliable streams | D-087 |
| Q82 | SQLite file per Universe | D-088 |
| Q83 | Client–server integration test in CI | D-089 |
| Q84 | Iso, north-up strategic views | D-090 |
| Q85 | Select and right-click strategic orders | D-091 |
| Q86 | Full MVP UI set | D-092 |
| Q87 | Positional hotkey grid | D-093 |
| Q88 | Milestones α–δ | D-094 |
| Q89 | Three parallel work streams | D-095 |
| Q90 | Out-of-scope list, no audio | D-096 |
| Q91 | MVP acceptance criteria | D-097 |
| Q92 | Acceleration and turn rates | D-101 |
| Q93 | Docking capacity counts ships | D-102 |
| Q94 | Ops-area radius 20 km | D-103 |
| Q95 | Rules file copied into each Universe | D-104 |
| Q96 | Ships steer around structures | D-105 |
| Q97 | Default season 8 weeks | D-106 |
| Q98 | Random Universe ID plus name | D-107 |
| Q99 | State-hash checkpoints in the journal, checked on replay | D-109 |
| Q100 | Journal kept for 48 h | D-110 |
| Q101 | Start kit as in doc 14 §12 | D-112 |
| Q102 | S3 movement starting values | D-113 |
| Q103 | Engagement ends after 30 s clear | D-114 |
| Q104 | S3 fleet details as built | D-115 |
| Q105 | Season end freezes the Universe, read-only | D-116 |
| Q106 | Catch-up after a lag of more than 1 minute | D-117 |
| Q107 | Screen-blend bloom by default | D-118 |
| Q108 | Phase-1 scene is illustrative; slot geometry with S4 | D-119 |
| Q109 | Finding the server | D-120 |
| Q110 | Certificate and token are new at every server start, | D-121 |
| Q111 | Privacy of `server.json` | D-122 |
| Q112 | Rejection reasons go on the wire | D-123 |
| Q113 | A second connection for the same commander replaces the first | D-124 |
| Q114 | New crate `nomad-link` | D-125 |
| Q115 | Harvesting and refining as in doc 14 §6 | D-126 |
| Q116 | Power as in doc 14 §7 | D-127 |
| Q117 | Power allocation | D-128 |
| Q118 | Command cap | D-129 |
| Q119 | Slot template | D-130 |
| Q120 | Peripheral slots | D-131 |
| Q121 | FB-2 is the storage module | D-132 |
| Q122 | MULE cargo is 2,000 units | D-133 |
| Q123 | Convoy routes | D-134 |
| Q124 | Claiming moves into S4 | D-135 |
| Q125 | "Build-out in 2–3 days of income" (D-070) is read against the holding's own output | D-136 |
| Q126 | Pacing deviations are measured, not pre-tuned | D-137 |
| Q127 | Harvester and extractor modules follow the peripheral rule | D-138 |
| Q128 | S5 is split | D-139 |
| Q129 | Sector maximum = 25% from the core + 75% from that sector's own pylon, | D-140 |
| Q130 | Every core has the same dome | D-141 |
| Q131 | Regeneration = allocated MW × 50 / 120 per second; | D-142 |
| Q132 | Ship hit radius is half the model's short side, | D-143 |
| Q133 | Auto-targeting | D-144 |
| Q134 | Return fire | D-145 |
| Q135 | No friendly fire | D-146 |
| Q136 | SHLD SURGE stays at 20% per use | D-147 |
| Q137 | FLAK SCREEN | D-148 |
| Q138 | Recovery of disabled structures | D-149 |

## A. Universe and time

**✅ Q1 — Time model.** *(Blocking: docs 10, 11, 14)*
Does the Universe run as one continuous real-time simulation at 1:1, with tactical combat on the seconds scale shown in the phase-1 HUD ("TTI 1.1 s")? Or is there a slower strategic clock, with real-time battles only inside holdings?
*Recommendation:* one continuous real-time simulation on a fixed tick (about 10 Hz). Calibrate timers in layers: seconds for tactical play, minutes for builds, hours for travel between systems. That gives a 24-hour Universe without a separate turn system.

**✅ Q2 — Universe lifecycle.** *(Blocking: doc 10; see W-04)*
Is there one forever-persistent Universe, or seasons (for example 8–12 weeks) that end in a win condition and a reset?
*Recommendation:* seasons (P-06).

**✅ Q3 — What does "up to 100 players" mean?** *(Blocking: docs 10, 20)*
Is it 100 commanders in one Universe, or 100 players online at once? What concurrency do you expect: 20–30% online is typical for the genre. Will several Universes run side by side?

**✅ Q4 — Topology.** *(Blocking: doc 10)*
- Is the phase-1 hierarchy (Galaxy › Arm › Sector › System › Orbit › Holding › Ops grid) the real gameplay structure, or are some levels only flavour?
- Is a Universe exactly one galaxy?
- Is travel between systems only through gates and lanes?
- Can a holding have more than one ops grid ("OPS GRID 0")?
- Please confirm that Kestrel-7 is a *system*. The top bar says "SECTOR KESTREL-7".

## B. Players, conflict and the Deputy

**✅ Q5 — Who are the hostiles?** *(Blocking: docs 12, 15)*
Are they other players (PvP), NPC factions such as raiders (PvE), or both? Is the siege fleet in the phase-1 scenario a player or an NPC? This changes the difficulty of W-01 by an order of magnitude: PvE-only makes offline defence a tuning problem, while PvP makes it a fairness problem.

**✅ Q6 — The Deputy's authority.** *(Blocking: doc 12)*
Which of these may the Deputy do on its own?

1. Continue existing build queues
2. Start new builds or spend resources
3. Repair
4. Re-route power, for example to shield priority
5. Use abilities with cooldowns (SHLD SURGE, FLAK SCREEN)
6. Launch and recall defenders
7. Pursue attackers beyond its own holding
8. Reinforce an ally's holding
9. Retreat or evacuate
10. Run convoys and logistics
11. Diplomacy

*Recommendation (P-01):* 1, 3, 4, 5, 6, 9 and 10 within owner-authored standing orders. Item 2 only from an owner-approved list. Never 7, 8 or 11. *Worked out in `12-offline-play-and-deputy.md` §5.1.*

**✅ Q7 — Offline losses.** *(Blocking: docs 11, 15)*
Is it acceptable for a player to lose their mother station, or any holding, while offline? If not, which protection do you want: reinforcement timers, vulnerability windows, loss caps, post-attack protection or allied cover?
*Recommendation (revised in round 2):* the clock-based stakes rule in `12-offline-play-and-deputy.md` §3–4, with allied cover. A holding's core can only be lost inside its owner's exposure window, after at least 12 h notice. P-05 is withdrawn.

**✅ Q8 — Deputy while online.** Can a player hand a holding or fleet to the Deputy while they are connected?
*Recommendation:* yes (P-01). Round 2 goes further, see Q22.

**✅ Q9 — Abandonment.** What happens to a player who stops logging in? Options: the Deputy runs the empire indefinitely, it decays, or after N days it becomes neutral or NPC-owned. With 100 slots, idle empires otherwise occupy space forever.

**✅ Q10 — Losing the mother station, and the name "Nomad".** If the mother station is destroyed, is the commander eliminated, do they respawn, or do they relocate? The repository is called *Nomad*. Is that the working title, and does it mean the mother station can move between systems? A mobile mother station would change the economy, the defence and the Deputy design substantially.

## C. MVP

**✅ Q11 — Does the Universe keep running in the MVP?** *(Blocking: docs 20, 30)*
When the single human closes the client, does the Universe continue, with a headless server and the Deputy taking over, or may it pause?
*Recommendation:* keep running (P-03). Otherwise R-02 and R-03 go untested until after the MVP.

**✅ Q12 — MVP gameplay scope.** *(Blocking: doc 30)*
- Which loops are in: harvesting, refining, building modules, ship production, fleet control, combat, travel between systems, diplomacy with AI players?
- What are the win and lose conditions?
- What session length are you targeting?

**✅ Q13 — Hosting and platforms.** *(Blocking: doc 20)*
- Will the MVP server run locally next to the client, or on a hosted machine (Azure or GCP)?
- Which client platforms: Windows only, Linux and macOS too, or the web (wgpu can target WebGPU)?

**✅ Q14 — Sequencing against phase 1.** Is the phase-1 visual slice still the first deliverable, with the MVP to follow? Or should phase 1 be folded into the MVP milestones?
*Recommendation:* run phase 1 as specified, and design the simulation, Deputy and server in parallel. The fixture/view-model is the seam between them.

**✅ Q15 — The AI players in the MVP.** *(Blocking: doc 13)*
- Are they fixed enemies, potential allies, or free agents with diplomacy?
- Do they play by the human rules with the same information, or are they allowed to cheat?
- Should some of them simulate an offline human, handing over to their Deputy on a schedule, so the MVP exercises human-vs-Deputy fights?

## D. Technical and terminology

**✅ Q16 — Scope of the World/Universe rule.** *Answered 2026-11-06 (D-008):* *World* is fine as a word. The only rule is that the total game space is called the Universe.

**✅ Q17 — Rust stack constraints.** Do you have preferences or constraints for any of these?
- ECS: `bevy_ecs` standalone, `hecs`, or our own
- Async runtime and transport: `tokio`, QUIC via `quinn`
- Persistence: SQLite for the MVP, PostgreSQL later
- Serialisation
- Hosting: a cloud you want to standardise on, or one you want to avoid

**✅ Q18 — Working title.** Is the game called *Nomad*? The phase-1 package uses "Voxel RTS".

## E. Offline play (round 2, from `12-offline-play-and-deputy.md`)

**✅ Q19 — Exposure window.** Does each commander choose one daily window (proposed: 4 h, contiguous) during which their holdings' cores can be besieged? Should changes take effect only after a delay (proposed: 48 h)? Should the window be per commander, as proposed, or per holding?

**✅ Q20 — Timings.** Are these the right order of magnitude?
- At least 12 h of notice between a breach and the siege.
- A 60 min siege clock.
- 48 h of immunity after a successful defence.

Also: should a commander's holdings come out of Reinforced at staggered times, so one window cannot be dogpiled?

**✅ Q21 — What a lost siege means.** What happens when the core falls: are the core modules destroyed, does the attacker capture the holding, or is the owner evicted, so the mother station has to relocate? This is tied to Q10 and to what "Nomad" means.

**✅ Q22 — Always-on Deputy.** Should the Deputy execute the commander's doctrine at all times, with online play adding direct control on top? Or should it take over only at disconnect, as R-03 currently reads?
*Recommendation:* always on (PR-4). It removes the handover discontinuity, and players see and tune the Deputy every session.

**✅ Q23 — Vacation mode.** Do you want one? Proposed limits:
- Up to 7 days per season.
- A 72 h activation delay, so it cannot be used to dodge an attack that is already known.
- No offensive actions while it is active.

## Asked during the round-2 review

**✅ Q24 — Offline accumulation (W-08).** At +312 alloy/min, 8 h offline earns about 150k alloy, roughly 7× the current stock. Should the economy use storage caps, uncapped income with costs scaled to match, or upkeep and decay? *Answered: storage caps (D-025).*

**✅ Q25 — License.** Proprietary, MIT OR Apache-2.0, or AGPL-3.0? *Answered: proprietary, all rights reserved (D-031).*

## F. Round 3 (answered)

**✅ Q26 — Staggered Reinforced exits.** If several of a commander's holdings are breached, they could all come out of Reinforced at the start of the same window and be dogpiled.
*Recommendation:* spread their exit times across the window in 60 min steps, in breach order. Every siege then still finishes inside the 4 h window, for up to 4 holdings.

**✅ Q27 — Alliances after the MVP.** The MVP has no diplomacy (D-026), but the phase-1 HUD shows allied commanders and relief fleets, and the stakes rule counts on allied cover. Should the full game have formal alliances (shared vision, the right to defend each other's holdings), or informal cooperation only? Not blocking for the MVP.

## G. Round 3, from `04-design-review.md` (answered)

**✅ Q28 — How the exposure window is set (RK-01).** Free choice invites time-zone tanking: picking 04:00–08:00, when enemies are asleep.
*Recommendation:* the window is the commander's most active 4 h over a rolling 14 days, moving at most 1 h per day; free choice only until 7 days of history exist. *Alternative:* restrict windows to a prime-time band in Universe time.

**✅ Q29 — Strategic time scale (RK-02).** Should each Universe have a time-scale setting that compresses strategic timers (notice, immunity, travel, builds, storage fill, season length) while combat and the tick stay at 1:1 (D-019)?
*Recommendation:* yes. Run the MVP in a 1× "persistent" mode and an accelerated mode for playtests.

**✅ Q30 — Headless simulation track (RK-02, RK-04).** Should we start a headless `nomad-sim` prototype in parallel with phase 1? It would contain a minimal economy, fleets, the siege lifecycle and a first Deputy, and serve as the pacing tool and the seed of the server. This refines D-028.
*Recommendation:* yes.

**✅ Q31 — Staging the MVP (RK-03).** Should the first MVP acceptance be 3 AI commanders plus NPC raiders in one sector, with the AI stack built for 9? Should vacation and derelict states be cut from the MVP?
*Recommendation:* yes to both.

**✅ Q32 — Information model (RK-05).** Should there be fog of war per commander based on sensor coverage, with last-known information in strategic views and the server sending only what each commander can see?
*Recommendation:* yes.

**✅ Q33 — Emergency jump details (RK-06).** When a mother station falls:
- Are its site modules destroyed, or captured?
- Where does it jump?
- What protection follows?

*Recommendation:* the modules are destroyed, and the attacker gains the cleared site. The station jumps to the commander's nearest owned holding, or else to a free site in a low-density sector. It then has 72 h of protection, and a comeback grant is scaled to how far the commander has fallen behind.

**✅ Q34 — Season carry-over (RK-07).** What survives a season reset?
*Recommendation:* blueprints (saved station layouts), doctrine presets and cosmetics. Not resources, holdings or fleets.

## H. Round 4, from `10-universe-structure.md` (answered)

**✅ Q35 — System control rule (doc 10 §6).** A commander controls a system with a strict majority of its sites as holdings (4 of 7). Only holdings with a completed core count, outposts do not. The system is Contested when nobody has a majority and two or more commanders own holdings there, and Open otherwise. During the season, control gives sensor coverage of the whole system.
*Recommendation:* adopt as written.

**✅ Q36 — Uninterruptible jump transit (doc 10 §4.2).** Transit between systems is a timer that cannot be interrupted. All interception happens inside systems, mostly at gate approaches.
*Recommendation:* adopt. It keeps the simulation simple and makes gates the chokepoints.

**✅ Q37 — Scale targets (doc 10 §3, §12).** About 15 sites per commander; 5–9 sites and 2–4 gates per system; sectors of 12–16 systems for 6–8 commanders; jump lanes of 40–70 min; at most 4 jumps across a sector, keeping every trip within a sector under 6 h. MVP stage 1: one sector of 9 systems plus a raider haven, with about 60 sites.
*Recommendation:* adopt as starting values, then tune them in the pacing simulation (D-037).

**✅ Q38 — Season galaxy (doc 10 §7).** Generate a new galaxy from a recorded seed each season. The topology is fully charted from the start (no exploration in the MVP), holding ownership is public, and everything else is under fog of war.
*Recommendation:* adopt.

**✅ Q39 — Late joiners (doc 10 §7, RK-11).** Dormant frontier sectors sized for 15% of the commander cap; 7 days of protection that ends on the first offensive action; a start grant scaled to how much of the season has passed; no joins in the final 14 days.
*Recommendation:* adopt.

**✅ Q40 — Phase-1 shield arcs (doc 10 §8).** The pylons sit exactly on the sector boundaries in phase-1 doc 01 §6, not at the centres of their arcs.
*Recommendation:* centre each arc on its pylon (N 180–270°, E 270–360°, S 0–90°, W 90–180°) and define grid north as azimuth 225°, which is screen-up in view 0. The siege scenario is unaffected. Also rename the camera clamp's "sector bounds" to "ops-area bounds".

## I. Round 5, from `11-time-and-simulation.md` (answered)

**✅ Q41 — Downtime and catch-up (doc 11 §7).** When the server starts after being down (in the MVP: whenever your PC was off), it fast-forwards through the missed time until it reaches the wall clock. Everyone is offline during catch-up, so the existing offline rules apply. NPC raiders start no new raids during catch-up. For exceptional long outages on a hosted server, an operator may freeze the Universe instead. This also confirms the provisional reading of D-036: whole-Universe fast-forward is allowed.
*Recommendation:* adopt.

**✅ Q42 — Persistence (doc 11 §6, §8).** Full snapshots every 5 min and on clean shutdown, plus a journal of all inputs (player commands, connect and disconnect events, operator commands) flushed at least once per second. On a crash, at most about 1 s of commands is lost; recovery is snapshot, then journal replay, then catch-up. Server upgrades always go through a clean-shutdown snapshot.
*Recommendation:* adopt.

**✅ Q43 — Determinism scope (doc 11 §5).** The same build, snapshot and journal produce bit-identical state. Stable `SimId`s; ordering by `SimId`; `f64` positions with the `libm` crate for transcendental functions; integer resources; seeded random streams per system and purpose; a determinism hash test in CI.
*Recommendation:* adopt.

**✅ Q44 — Simulation crate boundary (doc 11 §9).** `nomad-sim` is a pure library (`step(state, inputs) -> outputs`) with no clock, I/O, threads or networking. The Deputy and the AI run inside the deterministic step. A later `nomad-server` owns time, the network, persistence and catch-up.
*Recommendation:* adopt.

**✅ Q45 — When does building start?** *Answered: now, in parallel with design. The phase-1 client and the `nomad-sim` core start without game rules (D-051).*

## J. Round 6, from `15-combat-and-defence.md` (answered)

**✅ Q46 — Combat model (doc 15 §3–4).** 2D projectiles with travel time and swept hit tests; spread that narrows as lock builds (seeded random numbers); integer hull and damage; a counter triangle through damage multipliers: interceptors beat strike craft, strike craft beat frigates and structures, frigates beat interceptors.
*Recommendation:* adopt.

**✅ Q47 — Dome (doc 15 §5).** One-way: outgoing fire passes, hostile projectiles stop. Each sector has a strength pool regenerated by allocated power. Pylons are inside the dome and cannot be targeted separately. Every core projects a minimal dome (25% strength), and each pylon adds 25%.
*Recommendation:* adopt.

**✅ Q48 — Claims and outposts (doc 15 §8).** A MULE deploys a core kit; construction takes 2 h. An outpost under construction can be destroyed at any time (an exception to the disable-only rule), so claims can be contested.
*Recommendation:* adopt.

**✅ Q49 — Gates (doc 15 §9).** Arrivals scatter up to 2 km and get a 10 s jump shroud (cannot be targeted, cannot fire). Departure needs a 5 s spool-up. Armed hostiles within 5 km make a gate contested for convoys.
*Recommendation:* adopt.

**✅ Q50 — NPC raiders (doc 15 §10).** Raiders do zero damage to domes, so they can never breach or siege. They raid convoys, ships outside docks, outposts and peripheral structures.
*Recommendation:* adopt.

**✅ Q51 — Siege and capture details (doc 15 §6–7).**
- The attackers of a siege are the commanders who damaged the dome in the 10 min before the breach.
- Held: sectors restore to 50%.
- Capture goes to whoever dealt the most core damage. The core restores to 25%, modules transfer and build queues are cancelled.
- Docked ships emergency-jump to their owner's nearest holding.
- The new owner gets 48 h of protection.
- A fallen mother station's cleared site passes to the attacker as an outpost.

*Recommendation:* adopt.

**✅ Q52 — Time-to-kill targets (doc 15 §11).** Skirmish 30–90 s; fleet battle 3–8 min; breaching a full sector with a mid-size fleet 5–10 min; killing the core in a siege 15–25 min. These are the tuning goal; the stat numbers are only a starting point.
*Recommendation:* adopt the targets. Tune the numbers in the pacing simulation.

## K. Round 6, phase-1 implementation calls from M1 and the preview (answered)

**✅ Q53 — Grid fade.** M1 followed the spec's fade, which is about 1.6× dimmer than the reference render at mid distance. *Answered: match the reference (D-059).*

**✅ Q54 — Camera pivot.** The projection formula versus the screen centre. *Answered: keep the formula (D-060).*

**✅ Q55 — Star count.** Spec (about 200) or mockup (380)? *Answered: spec (D-061).*

**✅ Q56 — View names.** "VIEW NE" conflicts with north at screen-up. *Answered: name views by facing direction, N, W, S, E (D-062).*

**✅ Q57 — M1 defaults.** *Answered: accept them, and move tuning values to `scene_config.json` (D-063).*

## L. Round 7, from `14-economy.md` (answered)

**✅ Q58 — Where resources are stored (doc 14 §4).**
- Spending comes from one pool per commander.
- Holdings in a system that has a depot (the mother station, or any holding with storage) deliver straight to the pool.
- Other holdings fill a 2 h local buffer, which MULE convoys empty. Convoys can be raided and held by blockades.

*Recommendation:* adopt. The alternatives are a pool without convoys (raids and blockades lose meaning) and fully local stockpiles (too much logistics for the MVP).

**✅ Q59 — MVP resources (doc 14 §3).** ALLOY and CRYSTAL as stocks; ORE as an intermediate flow; POWER as a flow per holding; COMMAND as the fleet cap. MUNITIONS is left out of the MVP, and FB-2 becomes a second refinery.
*Recommendation:* adopt.

**✅ Q60 — Deposits (doc 14 §5).** 0–2 deposits per site (ore field or crystal vein), poor, standard or rich (×0.5, ×1, ×1.5), never running out, distributed by system kind. Deposit value: ore 1, crystal 3 (× richness).
*Recommendation:* adopt.

**✅ Q61 — Module placement (doc 14 §9.1).** Fixed build slots per site at the phase-1 elevation tiers (pylon, peripheral and inner slots), with trusses generated automatically. A blueprint is a slot assignment plus a build order.
*Recommendation:* adopt. Free placement needs collision and tier rules, placement UI and AI layout.

**✅ Q62 — Build queues and payment (doc 14 §9.2).** One queue per holding, plus one parallel ship lane per dry dock. Items are paid when they start and wait if the pool is short. Cancelling refunds 100% before start and 50% after.
*Recommendation:* adopt.

**✅ Q63 — Economic bonus for system control (doc 14 §11).** None. Control already decides the season and gives sensor coverage.
*Recommendation:* no bonus.

**✅ Q64 — Economy pacing targets (doc 14 §13).** Storage fills in 8–12 h; first expansion within 1 h; one new holding per 12–24 h early on; a full cap fleet takes about 24 h of mid-season income; building out a holding takes 2–3 days; late income is 3–4× starting income. These are the tuning goal; the numbers are starting values.
*Recommendation:* adopt.

**✅ Q65 — Repair, salvage and upkeep (doc 14 §10).** Free slow repair (1% per minute out of combat); the REPAIR ability costs 8 A/s for 80 hull/s; no salvage; no upkeep.
*Recommendation:* adopt.

## M. Round 8, from `16-information-model.md` (answered)

**✅ Q66 — Detection model (doc 16 §3).** Detection is binary within sensor range × target signature (Light 0.6, Medium 1.0, Heavy and Structure 1.5). A detected target is fully identified. No stealth, ECM or decoys in the MVP.
*Recommendation:* adopt.

**✅ Q67 — Sensor sources and ranges (doc 16 §3).**
- Holding core 40 km (phase-1 "PULSE · 40 KM").
- Ships: VX 15 km, LANCER 10 km, WARDEN 25 km, MULE 8 km; raiders 12 km and 30 km.
- System control gives coverage of the whole system.
- One shared picture per commander.

*Recommendation:* adopt as starting values.

**✅ Q68 — Visibility of fleets in transit (doc 16 §4).** Fleets in jump transit are invisible. Anyone covering the origin gate sees the departure, destination and arrival time (the phase-1 lane traffic). Arrivals are visible at the destination gate.
*Recommendation:* adopt.

**✅ Q69 — Public and private information (doc 16 §2).** Public: topology, ownership, control, standings and **each commander's exposure window**. Private: **online status**, doctrine, build queues, stockpiles. Siege announcements go to the owner and the commanders with breach credit.
*Recommendation:* adopt.

**✅ Q70 — Last-known memory (doc 16 §5).** Kept on the server per commander and included in snapshots. Fleets expire after 24 h without a sighting; structures stay until seen again or until ownership changes. Strategic views show the age of each entry.
*Recommendation:* adopt.

## N. Round 9, from `13-ai-commanders.md`, plus one client question (answered)

**✅ Q71 — AI architecture (doc 13 §3; adopts P-02).** Three layers:
- the Deputy, for every commander;
- the Strategist, for AI commanders only, playing through the same commands a human sends and writing its Deputy's doctrine before going offline;
- the raider director.

All three read only `CommanderView` and run inside the deterministic step.
*Recommendation:* adopt.

**✅ Q72 — How the Strategist decides (doc 13 §5).**
- Utility-scored goals, re-evaluated every 10 s: expand, build out, logistics, compose fleet, defend, raid, breach and siege, scout, retake.
- Each goal runs as a plan of ordinary commands.
- It commits to a siege only if it will be online at the siege start.
- Seeded personalities vary its play; it has no bias for or against the human.

*Recommendation:* adopt.

**✅ Q73 — Difficulty (doc 13 §7).** Easy, Normal and Hard change only competence: reaction time 1.0 / 0.6 / 0.3 s, command rate 1 / 2 / 4 per second, evaluation interval, decision noise, build-order quality and scouting. Never resource or information bonuses.
*Recommendation:* adopt, with Normal as the default.

**✅ Q74 — AI online schedules (doc 13 §6).** Seeded sessions, 2–3 a day and 1.5–4 h in total, at consistent times. While offline, only the Deputy runs. During catch-up, every AI commander is offline.
*Recommendation:* adopt.

**✅ Q75 — Raider director (doc 13 §8).**
- One raid per haven every 2–4 h.
- Raid strength is 30–50% of the target's fleet value in that system.
- Targets: peripheral assets, outposts and ships outside docks. No commander is raided twice within 6 h.
- Raids withdraw after 10 min or 50% losses. None start during catch-up.

*Recommendation:* adopt.

**✅ Q76 — Doctrine presets (doc 13 §9).** Fortress, Balanced and Trader, with the parameter values in the table. Balanced is the default.
*Recommendation:* adopt as starting values.

**✅ Q77 — AI acceptance criteria (doc 13 §10).** Determinism; at least 90% of income spent; 4 or more holdings by day 3 when unopposed; at least 60% of battles won using the counter triangle; at least 3 sieges and 1 capture in 14 days; nothing stuck; within budget; fairness enforced by build-time and runtime checks.
*Recommendation:* adopt.

**✅ Q78 — Windows graphics backend for the client.** wgpu is built with DX12, Vulkan and OpenGL ES. On Windows it will most likely choose Vulkan when a Vulkan driver exists, unless `WGPU_BACKEND` says otherwise.
*Recommendation:* make DX12 the default on Windows, falling back to Vulkan and then OpenGL ES. It is the platform's native API, it works with PIX, and it is closest to the owner's own DX12 work. Linux keeps Vulkan first.

## O. Round 10, from docs 20, 17 and 30 (answered)

**✅ Q79 — Process model (doc 20 §2).** A separate headless server. The client starts it in the background, and it keeps running after the client closes. "Start with Windows" is opt-in, as a per-user logon task. Localhost only.
*Recommendation:* adopt.

**✅ Q80 — Crates (doc 20 §3).** Add `nomad-types` (shared primitives), `nomad-protocol` and `nomad-server`. The client depends on protocol and types, never on `nomad-sim`.
*Recommendation:* adopt.

**✅ Q81 — Protocol (doc 20 §5).**
- QUIC via `quinn` with TLS, using a self-signed certificate pinned by the client.
- Reliable streams only in the MVP; `postcard` frames; exact protocol-version match.
- Commands answered with the tick they apply on, or a rejection.
- The commander's view sent as a snapshot plus deltas, at the doc 16 rates.

*Recommendation:* adopt.

**✅ Q82 — Persistence implementation (doc 20 §6).** `rusqlite` with bundled SQLite in write-ahead-log mode, one file per Universe in the per-user data directory. Snapshots are uncompressed in the MVP.
*Recommendation:* adopt.

**✅ Q83 — Testing (doc 20 §11).** Add a localhost client–server integration test to CI, Debug on both operating systems. Long AI and pacing runs are ignored tests run manually; no scheduled CI job in the MVP.
*Recommendation:* adopt.

**✅ Q84 — View levels (doc 17 §3–4).** TACTICAL, SYSTEM, SECTOR and GALAXY on F1–F4, with double-click to drill down. Strategic views use the same 2:1 iso style, north-up, with no rotation. GALAXY stays minimal in the MVP.
*Recommendation:* adopt.

**✅ Q85 — Orders in strategic views (doc 17 §5).** Select fleets in SYSTEM or SECTOR; right-click to move, attack or jump, with a route and arrival-time preview. Micro stays in TACTICAL.
*Recommendation:* adopt.

**✅ Q86 — UI beyond the phase-1 HUD (doc 17 §6).** Doctrine editor, Deputy status, away report, window and siege panel, build menu with slot diagram and blueprints, economy panel, fleet panel, season panel and start screen. In-client notifications, plus a Windows toast while the client runs.
*Recommendation:* adopt.

**✅ Q87 — Hotkeys (doc 17 §7; resolves phase-1 doc 07 item 1).** Positional command card on QWER / ASDF / ZXCV. Camera rotation on Alt+Q/E and mouse buttons 4/5. Pan with the arrow keys, screen edges and middle-mouse drag; WASD panning is dropped. Control groups on 1–9.
*Recommendation:* adopt.

**✅ Q88 — Milestones (doc 30 §3).** α headless game → β playable slice → γ stage 1, which is MVP acceptance (D-038) → δ stage 2, each with its acceptance criteria.
*Recommendation:* adopt.

**✅ Q89 — Order of work (doc 30 §4).** Three parallel streams: rules and AI (the critical path); server and network (N1–N2 can start now); client and UI (phase-1 M3–M6 now, live data later).
*Recommendation:* adopt.

**✅ Q90 — Out of scope for the MVP (doc 30 §6).** The consolidated list, including **no audio in the MVP**.
*Recommendation:* adopt.

**✅ Q91 — MVP acceptance criteria (doc 30 §5).**
- A full season runs in the harness, and a 14-day human playtest succeeds.
- The AI meets D-083.
- Pacing is within ±25% of D-058 and D-070.
- The offline experience is correct.
- Engineering criteria are met.

*Recommendation:* adopt.

## P. Round 11, from `plans/S3-movement-and-orders.md` and `plans/N2-server-shell.md` (answered)

**✅ Q92 — Acceleration and turn rates (S3 §5).** Starting values, since doc 15 gives only speeds: VX 60 m/s² and 90°/s; LANCER 40 and 60; WARDEN 15 and 20; MULE 15 and 25; raider strike craft 50 and 75; HSTL heavy cruiser 8 and 10.
*Recommendation:* adopt as starting values.

**✅ Q93 — Docking capacity (S3 §5).** The phase-1 fixture shows "DOCKED 14 / 24", but nothing designs it. Proposed: mother-station core 24 ships; other cores 8; each dry dock +6; frigates dock only where there is a dry dock.
*Recommendation:* adopt.

**✅ Q94 — Ops-area radius (S3 §5).** Confirm the 20 km working value from doc 10 §8. It drives cruise mode and the tactical view.
*Recommendation:* confirm.

**✅ Q95 — Rules data (S3 §4.1).** Unit and movement numbers live in a rules file embedded in the build, which a harness option can override, and are copied into the Universe at creation. A season keeps its rules (D-020); replay stays exact.
*Recommendation:* adopt.

**✅ Q96 — Structures on the ship plane (S3 §5).** The plane at z = 16 cuts through the core spindle, the pylons and FA-1.
*Recommendation:* modules that cross the plane are obstacles ships steer around, and ships keep a soft separation from each other.

**✅ Q97 — Default season length (N2 §5).** 8 weeks, matching the MVP acceptance run; `create` can set 8–12 weeks.
*Recommendation:* adopt.

**✅ Q98 — Universe ID (N2 §5).** A random 8-character base-32 ID plus an optional display name.
*Recommendation:* adopt.

## Q. Round 12, from the determinism review (answered)

**✅ Q99 — Detecting divergence on replay (doc 11 §5, §8).** A state hash is produced at a configurable interval, but nothing stores it or compares it during recovery. A determinism bug would change up to 5 minutes of replayed play without anyone noticing. Proposed: every 600 ticks (60 s), append the state hash to the journal; replay compares it, logs any mismatch as an error and carries on.
*Recommendation:* adopt.

**✅ Q100 — Journal retention (doc 11 §8).** The journal was kept only for the last 2 snapshots, about 10 minutes, so an incident from the night before could not be replayed even though hourly snapshots exist. Proposed: keep the journal for 48 h, matching the hourly snapshots. Entries written before an upgrade still need the old build, and a tool for replaying to a chosen tick is not yet specified.
*Recommendation:* adopt, with the 48 h value provisional until the journal volume is measured.
## R. Round 13, from building S3, N2, M3 and the station spread (answered)

**✅ Q101 — Season start kit (doc 14 §12).** S3 already builds the mother station's dry dock and the 3 VX + 2 MULE fleet from §12, and S4 will build the rest, but §12 is still labelled *Proposed*.
*Recommendation:* agree §12 as written.

**✅ Q102 — S3 movement starting values.** Doc 15 gives no numbers for these, so S3 uses them as *Proposed* entries in the rules file: ship separation 120 m (soft; two ships sharing a slot settle about 80 m apart), obstacle margin 60 m with a 4 s look-ahead, guard radius 3 km, the launch and docking point at the centre of the phase-1 CAP WING, and new fleets starting on *weapons free* (the Balanced doctrine, D-082).
*Recommendation:* adopt as starting values, tuned like every rules-file number.

**✅ Q103 — When an engagement ends (doc 15 §3.3).** Detecting a hostile within 20 km or taking fire drops a fleet to tactical speed, but nothing says when it may cruise again. S3 has the hook and no rule.
*Recommendation:* cruise resumes once no hostile has been detected within 20 km and the fleet has not taken fire for 30 s. The delay stops a fleet flipping between modes at the edge of detection range.

**✅ Q104 — Fleet details settled while building S3.** (a) Every docked ship counts as ready to launch, and docking a fleet is all-or-nothing. (b) "Class weapon range" for a mixed fleet is the shortest range among its armed ships; for the WARDEN that is its battery's 3 km. (c) Order positions are bounded by the farthest site or gate in the system plus 20 km. (d) A fleet entering an ops area at cruise speed sheds speed inside it, limited by its acceleration, rather than having to arrive at tactical speed. (e) Gate spool-up is per fleet: if any ship leaves the 2 km range, the whole fleet's spool-up is cancelled.
*Recommendation:* adopt all five as built.

**✅ Q105 — Season end on the server (D-020; N2).** When the season clock runs out, the server now logs a warning and keeps simulating, because nothing defines what happens next.
*Recommendation:* at the final tick, S8 fixes the standings and the winner. The Universe then stops advancing: the server stays up and serves the final state read-only until it is shut down, and a new season is a new Universe (D-020).

**✅ Q106 — Catching up after the PC sleeps (doc 11 §2; N2).** A sleeping PC suspends the server, and on waking the Universe is behind UTC. N2 treats a lag of more than one minute like downtime: journaled `CatchUpStart`, fast-forward with every commander offline, then `CatchUpEnd`. Smaller lags are run off by back-to-back live ticks.
*Recommendation:* adopt, with the one-minute threshold.

**✅ Q107 — Bloom composite (phase-1 doc 07 item 22; M3).** A plain additive bloom turns the pale cyan pylon emitters white; the reference renders keep them cyan with a halo. M3 therefore uses a screen composite by default (`frame + glow × (1 − frame)`), which still only adds light. `"additive"` stays available in `scene_config.json`.
*Recommendation:* keep screen as the default.

**✅ Q108 — Inner modules outside the dome (D-067, doc 14 §9.1, doc 15 §5; found with D-111).** Inner slots, which hold the factories and the dry dock, are supposed to lie inside the dome. In the phase-1 scene they never did: with the dome circle at radius 36 voxels (1.26 km) around the station centre, 0% of FB-2, 16% of FA-1 and 11% of the dry dock were inside it, and after the spread almost none are. Covering them would take a radius of about 64 voxels, which breaks doc 15's 1.26 km and would swallow the solar array, which D-100 wants visibly outside.
*Recommendation:* keep the dome radius and treat the phase-1 scene as illustrative, not as a slot layout. The real slot geometry (inner slots inside the dome, outer slots outside) is designed with S4 and module placement (D-067), and the phase-1 scene is brought into line then.

## S. Round 14, from `plans/N3-transport-and-sessions.md`, `plans/S4-economy.md` and `plans/S5-combat.md` (answered)

The full questions, with alternatives, are in the plans' question sections. The owner chose every recommendation.

**✅ Q109** → D-120: **Finding the server:** it listens on 127.0.0.1 on a port the OS chooses, and writes the port, its certificate fingerprint and the session token to `<universe>/server.json`, which counts as stale while the Universe lock is free.

**✅ Q110** → D-121: **Certificate and token are new at every server start,** published only in `server.json`, which the client reads on each connect. Amends doc 20 §5 and §9 ("on first start").

**✅ Q111** → D-122: **Privacy of `server.json`:** mode 0600 on Linux; on Windows the per-user ACL of `%LOCALAPPDATA%`. The server warns when the data directory lies outside the user profile. No `unsafe` Win32 code.

**✅ Q112** → D-123: **Rejection reasons go on the wire** as a wire enum in `nomad-types`. "Unknown", "not yours" and "not visible" collapse into one `NotFound`, so probing reveals nothing about unseen entities (doc 16 §6). The server maps validator reasons to it, with an exhaustive test. Protocol version 2.

**✅ Q113** → D-124: **A second connection for the same commander replaces the first** (`Bye { Replaced }`), without a disconnect/connect pair in the simulation.

**✅ Q114** → D-125: **New crate `nomad-link`:** the client side of the connection (connect, pinning, handshake, channels, reconnect), used by the game client (C7) and the integration tests. Amends D-086's crate list.

**✅ Q115** → D-126: **Harvesting and refining as in doc 14 §6:** refining is local to the holding, one harvester per ore field, a 1 h ore buffer, and power-limited producers slow in proportion.

**✅ Q116** → D-127: **Power as in doc 14 §7:** supply and demand per holding, never transferred between holdings, with the values in its table.

**✅ Q117** → D-128: **Power allocation:** fixed draws (pylon base, turrets, active abilities) are all-or-nothing and served first; scalable draws (regeneration, production, the building dock) share by routing-profile tiers, proportionally within a tier. An ability needs its full draw free to start.

**✅ Q118** → D-129: **Command cap:** 120 from the mother station, +10 per other completed holding, at most 300 (doc 14 §8). At 5 holdings that is 160, not the 200 doc 14 §13's rough check assumed.

**✅ Q119** → D-130: **Slot template:** every core projects the same 36-voxel dome; 4 pylon slots; inner slots 6 standard + 1 dock at the mother station and 4 + 1 elsewhere; 1 solar slot outside the dome. This is the slot geometry D-119 left to S4.

**✅ Q120** → D-131: **Peripheral slots:** one solar slot plus one deposit slot per deposit, for its harvester or extractor. Supersedes D-067's "1–2 peripheral slots", which could not hold two deposits and a solar array (D-098).

**✅ Q121** → D-132: **FB-2 is the storage module** (it carries a container rack). Supersedes D-065's clause making FB-2 a second refinery; D-112's one-refinery start kit stands.

**✅ Q122** → D-133: **MULE cargo is 2,000 units** (1 alloy or 1 crystal = 1 unit); a core kit fills the hold.

**✅ Q123** → D-134: **Convoy routes:** a convoy order is a looped list of own holdings ending at a depot. MULEs load buffered alloy and crystal in proportion up to capacity and unload into the pool; overflow at the pool is lost, as at the source.

**✅ Q124** → D-135: **Claiming moves into S4:** core kits, delivery, the 2 h outpost and completion as a holding. S6 keeps the outpost combat exception (D-054) and capture. Amends doc 30 §2.

**✅ Q125** → D-136: **"Build-out in 2–3 days of income" (D-070) is read against the holding's own output** (600 A/h for a standard holding): about 2.4 days.

**✅ Q126** → D-137: **Pacing deviations are measured, not pre-tuned:** S4 and S5 build with the starting values; the harness measures every D-070 and D-058 target at α, and then only numbers are tuned (ship costs, yields, hull).

**✅ Q127** → D-138: **Harvester and extractor modules follow the peripheral rule:** disabled outside the owner's window, destroyable inside it. Doc 12 §3's "harvesters" means harvesting craft, of which the MVP has none.

**✅ Q128** → D-139: **S5 is split:** S5a (ship combat: weapons, lock, projectiles, multipliers, auto-targeting) is built in parallel with S4; S5b (dome sectors, regeneration, turrets, abilities, module damage) follows S4. Amends doc 30 §4.

**✅ Q129** → D-140: **Sector maximum = 25% from the core + 75% from that sector's own pylon,** so four pylons give full strength and a missing pylon leaves its sector at 25%. Supersedes D-053's "each pylon adds 25% to its sector", which never reached doc 15 §12's full strength.

**✅ Q130** → D-141: **Every core has the same dome:** radius 36 voxels (1.26 km), 150,000 per full sector, 50/s regeneration. Holdings differ only through power and pylons.

**✅ Q131** → D-142: **Regeneration = allocated MW × 50 / 120 per second;** the maximum rate and the MW drawn scale with the sector's maximum (a 25% sector regenerates at most 12.5/s and draws at most 30 MW).

**✅ Q132** → D-143: **Ship hit radius is half the model's short side,** as rules values (VX ≈ 90 m, LANCER and WARDEN ≈ 120 m, MULE ≈ 50 m, HSTL ≈ 190 m); TTK is tuned through hull at α. A smaller collision radius stays an option at A6 if fights feel too deterministic.

**✅ Q133** → D-144: **Auto-targeting:** with no explicit target under *weapons free*, each weapon takes the hostile in range with the highest multiplier for it, then the nearest, then the lowest `SimId`, and keeps it while valid. Explicit targets (player, Deputy, AI) override it.

**✅ Q134** → D-145: **Return fire:** a fleet engages only hostiles that damaged one of its ships, or one of its commander's structures within its weapon range, in the last 30 s (D-114's window).

**✅ Q135** → D-146: **No friendly fire:** projectiles pass through friendly ships, and flak damages only hostile targets.

**✅ Q136** → D-147: **SHLD SURGE stays at 20% per use** as a starting value. D-058's breach target is measured with the Deputy's default ability use; the core-kill target with the defender's turrets destroyed or absent.

**✅ Q137** → D-148: **FLAK SCREEN:** for 10 s the holding fires one burst per second at each hostile Light or Medium target within 2 km of the dome centre, at most 8 bursts per second, each with the WARDEN's flak values (30 damage, 150 m radius).

**✅ Q138** → D-149: **Recovery of disabled structures:** back online once no hostile has been inside the holding's ops area (20 km) for 10 min, at 10% hull, then repaired normally (D-071); REPAIR brings it online at once.
