# 02 — Open questions

Status: **All questions answered (Q1–Q40) on 2026-11-06.** Answers are recorded in `03-decision-log.md`, and the question is then marked ✅ here. The full original text of each question stays below for history. Recommendations are mine, and the owner is free to overrule them.

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
