# 02 — Open questions

Status: **All questions answered (Q1–Q243) on 2026-11-07.**

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
| Q139 | Fixed default port with bind and port overrides | D-150 |
| Q140 | Persistent certificate and token, `connect-info` | D-151 |
| Q141 | Containers are a developer mode; external-server setting | D-152 |
| Q142 | What counts as activity (D-032) | D-154 |
| Q143 | Window granularity and when it moves | D-155 |
| Q144 | Freeze the window during a pending siege | D-156 |
| Q145 | The initial window and the first 7 days | D-157 |
| Q146 | Stagger slots and the 12 h notice (D-015 against D-033) | D-158 |
| Q147 | A sector at 0 during protection (doc 12 §3 against the protection rules) | D-159 |
| Q148 | Who counts as online at the siege start | D-160 |
| Q149 | Attackers who go offline during the siege | D-161 |
| Q150 | Which orders lapse after the grace (doc 12 §5.3, PR-3) | D-162 |
| Q151 | Who may damage the core and capture (D-057 against doc 15 §7) | D-163 |
| Q152 | Where a fallen mother station goes (conflict in D-034) | D-164 |
| Q153 | "A free site in a low-density sector" in a one-sector MVP (D-034) | D-165 |
| Q154 | How an emergency jump travels | D-166 |
| Q155 | The comeback grant (D-034) | D-167 |
| Q156 | What a capture cancels and keeps | D-168 |
| Q157 | Disabled peripheral structures inside the window (doc 12 §3) | D-169 |
| Q158 | Are protections public? | D-170 |
| Q159 | Catch-up minutes in the activity history | D-171 |
| Q160 | Split S6 and order it against S5b | D-172 |
| Q166 | When the sensors phase runs | D-173 |
| Q167 | Detecting part of a fleet | D-174 |
| Q168 | Detecting holdings and modules | D-175 |
| Q169 | What carries sensors | D-176 |
| Q170 | What last-known memory stores | D-177 |
| Q171 | Clearing stale fleet entries | D-178 |
| Q172 | When a departure is observed | D-179 |
| Q173 | An Attack target that drops out of detection | D-180 |
| Q174 | NPC raiders and memory | D-181 |
| Q175 | Where each client's view is computed | D-182 |
| Q176 | What the 10 Hz tier covers | D-183 |
| Q177 | How rich view entries are | D-184 |
| Q178 | How combat effects reach the client | D-185 |
| Q179 | How the client tells the server what it views | D-186 |
| Q180 | Delta consistency | D-187 |
| Q190 | Where standings are computed, and when | D-191 |
| Q191 | When the early win triggers | D-192 |
| Q192 | "Whoever reached the final count first" (D-041's third tie-break) | D-193 |
| Q193 | A tie after every tie-break | D-194 |
| Q194 | The final tick in the simulation | D-195 |
| Q195 | The read model | D-196 |
| Q196 | A warning one system before the early win | D-197 |
| Q197 | When S8 is built | D-198 |
| Q200 | How AI code is kept from reaching `UniverseState` | D-199 |
| Q201 | How AI commands enter the step | D-200 |
| Q202 | The competence cap's mechanics | D-201 |
| Q203 | How the AI learns about events | D-202 |
| Q204 | Where AI evaluation runs, and its budget | D-203 |
| Q205 | AI presence before A5 | D-204 |
| Q206 | The build order towards α | D-205 |
| Q207 | Doc 13 §4 as the Deputy's rules | D-206 |
| Q208 | Fleet roles and the default role | D-207 |
| Q209 | What the replacement list covers, and its budget window | D-208 |
| Q210 | What "share of the fleet kept as garrison" means | D-209 |
| Q211 | Convoy evacuation and holding | D-210 |
| Q212 | Where AI parameters live | D-211 |
| Q213 | The goals at A2 | D-212 |
| Q214 | What Expand optimises | D-213 |
| Q215 | Plans and the online session | D-214 |
| Q216 | Built-in blueprints | D-215 |
| Q217 | Depots or convoys | D-216 |
| Q218 | How late income is bounded | D-217 |
| Q219 | How α measures pacing with real AI | D-218 |
| Q220 | Which parts of the raider director are rules and which are AI | D-219 |
| Q221 | Fleet value, and a floor | D-220 |
| Q222 | Where raid groups come from, and where they go | D-221 |
| Q223 | When the 10 minutes start | D-222 |
| Q224 | Raiders and peripheral structures inside the owner's window | D-223 |
| Q225 | An outpost's hull while it is built | D-224 |
| Q226 | What "raids happen" means at α | D-225 |
| Q230 | The client's technology | D-238 |
| Q231 | The language of the HTML UI | D-239 |
| Q232 | Where the phase-1 HUD (M6) is drawn | D-240 |
| Q233 | The transport | D-241 |
| Q234 | The QUIC endpoint | D-242 |
| Q235 | Serving the client | D-243 |
| Q236 | The guard without a login | D-244 |
| Q237 | The process model in Docker Desktop | D-245 |
| Q238 | Fonts | D-246 |
| Q239 | Browsers | D-247 |
| Q240 | Keys and fullscreen in the browser | D-248 |
| Q241 | Notifications | D-249 |
| Q242 | Build, CI and tests | D-250 |
| Q243 | Where MP-1 sits | D-251 |

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
## T. Round 15, from the Docker Desktop trial (answered)

*Derived* from a trial of `nomad-server` in Docker Desktop on 2026-11-06. N2's server already runs in a Linux container (D-029), with a `Dockerfile` in the repository. There is no networking yet. These three gaps appear once N3 adds it, because doc 20 assumes the client and server share one Windows user's machine and files. Settle them before N3 starts. *(Renumbered from Q109–Q111 on merge: those numbers were already taken by the N3 plan's questions, answered as D-120 to D-125. Q139 and Q140 interact with D-120 and D-121.)*

**✅ Q139 (D-150) — Bind address and port (doc 20 §2; D-085, D-087; N3).** The server is specified to bind to 127.0.0.1. Inside a container that address cannot be reached through a published port. No design document names a port either. QUIC through Docker Desktop's UDP forwarding is also untested, and cannot be tested before N3.
*Recommendation (Proposed):* add a bind-address setting (`--bind`, or `NOMAD_BIND`) and fix a default UDP port in N3. The default address stays 127.0.0.1, so the MVP does not change. The image sets 0.0.0.0, and the host publishes the port on loopback only (`-p 127.0.0.1:PORT:PORT/udp`), which keeps D-085's "localhost only".

**✅ Q140 (D-151) — Getting the token and certificate to the client (doc 20 §5, §9; D-087; N3).** The client reads its token from the per-user data directory and pins the fingerprint of the server's self-signed certificate. With the server in a container, those files sit in a volume the client cannot read. A shared bind mount would expose them, but user-only file permissions mean little across the Windows file share, and the Universe database should stay off that share.
*Recommendation (Proposed):* add an operator command that prints the address, token and certificate fingerprint (for example `docker exec nomad nomad-server connect-info`), and let the client import them once into its own per-user configuration. The certificate and token stay in the data directory, so they survive an image upgrade. Hosting will need the same step. The cost is that the token appears in a terminal.

**✅ Q141 (D-152) — The client and a server it did not start (doc 20 §2, §7; D-085; N5).** The client starts the server if none is running. With the server in a container, the client cannot tell that from a server that is missing, so it could start a second, Windows server with a different Universe. "Shut down Universe" also misbehaves: under `--restart unless-stopped` the container restarted within a second of a clean exit, so the Universe would not stay stopped. "Start the Universe when I log in to Windows" does not apply either.
*Recommendation (Proposed):* keep the MVP on the Windows process model of D-085 and treat the container as a developer and soak-test mode, not a player-facing option. Add an explicit client setting for an external server: with it set, the client never starts a server and hides "Shut down Universe" and the logon option, and the operator stops the server with `docker stop`. Whichever way this is answered, record it as a new decision extending D-085.

## U. Round 16, from `plans/S6-offline-and-sieges.md`, `plans/S7-sensors.md` and `plans/N4-replication.md` (answered)

The full questions, with alternatives, are in the plans' question sections; Q161–Q165 and Q181–Q189 were not used. The owner chose every recommendation.

**✅ Q142** → D-154: **What counts as activity (D-032):** A minute counts when the commander is online at the 60 s sample (connected, or scheduled online for an AI). An idle open client counts. The 1 h/day drift and the 14-day history limit the effect, and a window in hours when the player isn't there only hurts that player.

**✅ Q143** → D-155: **Window granularity and when it moves:** 15-minute buckets over 14 UTC days. The window moves only at its own end, never while open, towards the best 4 h by at most 1 h per day. Ties keep the current window, then prefer the nearest start, then the earliest.

**✅ Q144** → D-156: **Freeze the window during a pending siege:** The window does not move while any of the commander's holdings is Reinforced or under siege, as D-032 freezes history during vacation.

**✅ Q145** → D-157: **The initial window and the first 7 days:** Set at creation: the human's from `create --window`, by default the client's local 19:00–23:00; an AI's from its schedule. A `SetExposureWindow` command may change it during the first 7 days, taking effect when the window is next closed. After that only activity moves it.

**✅ Q146** → D-158: **Stagger slots and the 12 h notice (D-015 against D-033):** The exit is the earliest slot (window start + 0, 1, 2 or 3 h) at least 12 h after the breach and after the commander's earlier-breached exits. Settles D-015 against D-033.

**✅ Q147** → D-159: **A sector at 0 during protection (doc 12 §3 against the protection rules):** Sectors can still be worn to 0, keep blocking and regenerate, and no breach happens. Once protection ends, the next hostile hit on a sector at 0 is a breach. Settles doc 12 §3 against the protection rules.

**✅ Q148** → D-160: **Who counts as online at the siege start:** Connected or inside the 120 s grace; an AI by its schedule. A network blip at the start doesn't hand the defender a free Held.

**✅ Q149** → D-161: **Attackers who go offline during the siege:** The clock runs on. Their direct orders lapse after the grace, and doctrine never attacks a core, so their assault stops in practice.

**✅ Q150** → D-162: **Which orders lapse after the grace (doc 12 §5.3, PR-3):** All direct fleet orders lapse and fleets fall back to doctrine (A1; until then station-keep). Economy queues and convoy routes are doctrine and continue.

**✅ Q151** → D-163: **Who may damage the core and capture (D-057 against doc 15 §7):** Any commander hostile to the owner can damage modules through the open sector, and capture goes to the most core damage among all of them. D-057's tie-breaks already deter last-second sniping. NPC raiders deal no damage to modules inside the dome, so they never take part in sieges (D-056). Settles D-057 against doc 15 §7.

**✅ Q152** → D-164: **Where a fallen mother station goes (conflict in D-034):** A free site in the system of the commander's nearest holding by route time; if there is none, the free site in the least-held claimable system (Q153); if there is none, it station-keeps by its nearest holding until a site frees up. Supersedes D-034's "jumps to the nearest owned holding".

**✅ Q153** → D-165: **"A free site in a low-density sector" in a one-sector MVP (D-034):** The free site in the claimable system with the fewest holdings. Ties go to the shortest route from the fallen site, then the lowest `SiteId`.

**✅ Q154** → D-166: **How an emergency jump travels:** An uninterruptible transit along the fastest route, lasting that route's time (in-system legs at cruise speed, spool-ups and lanes), untargetable and invisible like lane transit. On arrival it docks if there is capacity, otherwise it station-keeps by the core.

**✅ Q155** → D-167: **The comeback grant (D-034):** The cost of one core kit for each holding the commander is behind the standings leader after the fall; at least 1 kit and at most 3; paid into the pool, ignoring the storage cap.

**✅ Q156** → D-168: **What a capture cancels and keeps:** Queued and running builds at the holding are cancelled without refund. Its local buffers (ore, and alloy waiting for a convoy) transfer with it. The old owner's convoys to it hold or re-route by doctrine (S4).

**✅ Q157** → D-169: **Disabled peripheral structures inside the window (doc 12 §3):** Disabled structures stay targetable, and a hit inside the window destroys one.

**✅ Q158** → D-170: **Are protections public?:** Each holding's `breachable_from` is public, like the window (D-075).

**✅ Q159** → D-171: **Catch-up minutes in the activity history:** Record catch-up minutes as offline for everyone, consistent with D-047. The windows then follow the hours in which play can actually happen.

**✅ Q160** → D-172: **Split S6 and order it against S5b:** Build S6a (commander records, activity and windows, presence, grace and lapse, announcements infrastructure) in parallel with S5b. S6b (defence states, capture, emergency jumps, protections, peripheral outcomes, harness sieges) follows S5b.

**✅ Q166** → D-173: **When the sensors phase runs:** All observers at once on every fifth tick, so each half second has one consistent picture; stagger only if the harness shows a spike. At MVP scale a pass costs well under 1 ms.

**✅ Q167** → D-174: **Detecting part of a fleet:** Detection per ship, as doc 16 says. A fleet appears when any member is detected, and lists only the members that are detected; its composition is partly known until all are in range.

**✅ Q168** → D-175: **Detecting holdings and modules:** The holding is one target at its dome centre with signature 1.5 (Structure); detecting it reveals all its modules, their hull and state.

**✅ Q169** → D-176: **What carries sensors:** Completed holding cores and ships in space, including ships under the arrival shroud, plus system control. Not docked ships (the core already covers more), not fleets in transit (doc 16 §4), not outposts under construction, not modules.

**✅ Q170** → D-177: **What last-known memory stores:** One entry per fleet (composition by class, hull % per class, position, heading, velocity, tick seen) and one per holding (observed modules with kind, slot, hull % and state, tick seen). Narrows D-076's "every entity".

**✅ Q171** → D-178: **Clearing stale fleet entries:** Drop a fleet entry as soon as the commander's current picture covers its remembered position (for its largest remembered signature) and the fleet is not detected. The 24 h expiry still applies otherwise. Extends D-076's 24 h expiry.

**✅ Q172** → D-179: **When a departure is observed:** An observer sees the departure if a ship of the departing fleet was in its visibility set at the last pass before the fleet entered transit (always within the 5 s spool-up). The record holds the fleet's identity and composition as seen, destination and arrival tick, and expires when the arrival is observed or 10 min after the arrival tick.

**✅ Q173** → D-180: **An Attack target that drops out of detection:** The fleet continues to the target's last-known position and holds there; the order completes with a `TargetLost` event if the target is not detected again on the way.

**✅ Q174** → D-181: **NPC raiders and memory:** NPC factions have visibility sets and lane traffic, but no last-known memory; the raider director (A3) works from public data and live sensing.

**✅ Q175** → D-182: **Where each client's view is computed:** After each step, the simulation thread extracts each connected commander's due view through S7's `CommanderView`, and the network thread only diffs and sends. One visibility implementation serves the AI and replication, and the cost scales with connected clients, not the Universe. Amends doc 20 §4 ("change sets, visibility sets").

**✅ Q176** → D-183: **What the 10 Hz tier covers:** The focus tier is the whole system being viewed, in TACTICAL or SYSTEM; other systems with presence at 2 Hz; the galaxy summary every 5 s. On localhost the extra traffic does not matter. Amends doc 16 §6 and doc 20 §5 ("the ops area being viewed").

**✅ Q177** → D-184: **How rich view entries are:** Typed entries per kind now, carrying doc 16's identification (class, owner, hull %, fleet membership), position, heading and velocity, and in `own` the private details that the β views need (orders, queues, power, cargo, economy; U1–U3). Fields for later packages (S6 deadlines, S8 standings) are added in their packages. Protocol version 3 with N4.

**✅ Q178** → D-185: **How combat effects reach the client:** In the focus tier, events for projectiles fired and ended since the last delta, sent only when the shooter or target is own or sensed; the client simulates the straight flight between them.

**✅ Q179** → D-186: **How the client tells the server what it views:** A `SetFocus { focus }` message (TACTICAL site, SYSTEM, SECTOR or GALAXY), rate-limited, not journaled; it only changes rates, never visibility. Until the client sends one, the focus is the mother station's system.

**✅ Q180** → D-187: **Delta consistency:** Each delta carries `since`, the tick of the previous view message; the client reconnects into a fresh snapshot if the chain breaks. No periodic full snapshots.

## V. Round 17, from `plans/S8-season-end.md` and the AI plans `A1`–`A3` (answered)

The full questions, with alternatives, are in the plans' question sections; Q198, Q199 and Q227–Q229 were not used. The owner chose every recommendation. Four further decisions from the S5b and S6a builds and the AI plans' conflict list are D-226 to D-229.

**✅ Q190** → D-191: **Where standings are computed, and when:** In the simulation state, recomputed in phase 9 on any tick where completed holdings or their owners changed, with the control map cached there. The *since* tie-break needs this tick precision, and S6b and the AI need the standings inside the step.

**✅ Q191** → D-192: **When the early win triggers:** The season ends on the first tick on which a commander controls at least ⌈60% × claimable systems⌉ (6 of 9), and that tick is the final tick (D-116). Control only changes through visible, slow events (2 h outposts, scheduled sieges), so there is time to react before the decisive tick.

**✅ Q192** → D-193: **"Whoever reached the final count first" (D-041's third tie-break):** Among commanders tied on systems, holdings and deposit value, the one whose triple has been unchanged since the earliest tick wins: they reached the tied position first.

**✅ Q193** → D-194: **A tie after every tie-break:** Joint winners.

**✅ Q194** → D-195: **The final tick in the simulation:** `SeasonConfig` carries the final tick, set at creation from the season length (D-106). The step that computes it fixes the result, views show the season clock, and the server stops when the simulation reports a result, at the full length or at an early win.

**✅ Q195** → D-196: **The read model:** An append-only `standings_history` (a row per commander per change) and a `season_result` written once, both projections that are re-synced after recovery; `status` prints them. Changes are rare, so the history is tiny, and it serves the season panel and the away report's rank changes.

**✅ Q196** → D-197: **A warning one system before the early win:** A public announcement when a commander reaches threshold − 1, and another when they drop back. It reveals nothing new (control is public, D-075), but it makes the threat visible. An offline player then learns of it from the away report and notifications instead of finding the season over.

**✅ Q197** → D-198: **When S8 is built:** Now, in parallel with S5b and S6a. It needs only built code (holdings, control, the server's season end), and S6b's comeback grant (D-167) needs its standings leader.

**✅ Q200** → D-199: **How AI code is kept from reaching `UniverseState`:** A `nomad-ai` crate with all AI logic. It depends only on `nomad-types` and defines a `View` trait, which `nomad-sim` implements over `CommanderView`, and `nomad-sim` calls the AI from its phases. Cargo's ban on dependency cycles then guarantees the AI cannot name `UniverseState`. Amends D-086, as doc 11 §9 anticipated. Amends D-086's crate list.

**✅ Q201** → D-200: **How AI commands enter the step:** AI layers write to a per-owner outbox in the state with a `not_before` tick. Phase 1 of a later tick applies due AI commands after the tick's external inputs, in `(owner, layer, sequence)` order, through the same `execute`. Deputy commands to a fleet under direct control are dropped when applied (RK-10).

**✅ Q202** → D-201: **The competence cap's mechanics:** Latency 10, 6 or 3 ticks by difficulty, and 3 ticks for a human's Deputy, counted from the triggering event. At most the difficulty's command rate (1, 2 or 4 per second; 4 for a human's Deputy) in any 10 consecutive ticks, shared by all of a commander's AI layers. Waiting commands drain oldest first, Deputy before Strategist, and expire after 10 s.

**✅ Q203** → D-202: **How the AI learns about events:** A per-owner inbox in the state, filled from each tick's outputs with only what the owner may know (own damage coalesced per entity and attacker, own losses, own fleet and economy events, addressed announcements, public season events, its AI commands' outcomes), kept for 60 s and read through `View`.

**✅ Q204** → D-203: **Where AI evaluation runs, and its budget:** The Strategist in phase 3, staggered by commander over its interval; Deputies and raid-group control in phase 4 at 2 Hz, staggered by `SimId`; raid launches on a phase-2 timer. Budgets in a release build at stage 1: the Strategist's mean ≤ 5 ms per evaluation (D-083), bounded by deterministic work counters, never by time; Deputies and raid control together ≤ 25% of a 500× fast-forward tick (about 50 µs mean). The harness times `nomad-ai` directly and compares tick times with the AI on and off.

**✅ Q205** → D-204: **AI presence before A5:** Move D-080's seeded schedule generator into A2, running in the simulation from the seed (not journaled). It drives S6a's presence and the AI's initial window (D-157), so the Strategist knows when its session ends. `SetPresence` stays as a harness override. A5 keeps personalities, difficulty and schedule tuning.

**✅ Q206** → D-205: **The build order towards α:** S8 now, in parallel with S5b and S6a; S7 after S6a, in parallel with S6b; A1a (crate, outbox, gate, inbox, doctrine, direct control, rebuilding, convoy rerouting) after S6a; A1b (threat, alert, engagement, abilities, convoy evacuation) after S7 and S5b; A2 after A1a and S7; A3 after S6b, S7 and A1a, in parallel with A2. Amends D-095's serial order. Amends D-095's order of work.

**✅ Q207** → D-206: **Doc 13 §4 as the Deputy's rules:** Adopt it, with the details in §5: alert on a hostile in the zone or fire on an asset in the zone, ending 60 s later; alert profile, garrison launch and ability triggers from the preset; target priority as explicit attacks; leash from the role's anchor; retreat by fleet hull; convoy safety. Doc 13 §4 becomes Agreed with these details.

**✅ Q208** → D-207: **Fleet roles and the default role:** New ships join the garrison of the holding that built them (or its rally fleet). A garrison docks at its holding, launches on alert, engages in the zone up to the leash and docks 60 s after the alert. A reserve stays docked. Patrol and escort run as `Patrol` and `Guard` orders, with the leash measured from the route or the escortee. MULEs not on a convoy are reserves.

**✅ Q209** → D-208: **What the replacement list covers, and its budget window:** Listed ships and destroyed modules (rebuilt in the same slot, on by default per holding). Budget: the preset's share of each resource's income over the last 24 h, against replacement spending over the same rolling 24 h. Anything that does not fit is reported as deferred.

**✅ Q210** → D-209: **What "share of the fleet kept as garrison" means:** A Strategist parameter. The Strategist commits at most (1 − share) of its fleet value to plans away from its holdings, and when it writes doctrine before going offline every uncommitted fleet becomes a garrison. For a human it only sets the doctrine editor's default role for new fleets.

**✅ Q211** → D-210: **Convoy evacuation and holding:** When a sensed hostile is within the evacuation radius of a convoy or its next stop, the convoy docks at the nearest own holding on its route and resumes 60 s after no hostile is within the radius. At a contested gate it reroutes if another route exists, otherwise it holds docked at its last stop.

**✅ Q212** → D-211: **Where AI parameters live:** An `ai` section in the rules file (D-104): presets, the difficulty table, the human Deputy's competence, inbox and outbox timings, Strategist hysteresis and noise, blueprints, raid parameters. They are copied into the Universe, so a season keeps its AI, replay stays exact, and A6 tunes them with `--rules`.

**✅ Q213** → D-212: **The goals at A2:** Those three, plus a stand-in *Garrison* goal that fills the command cap with the preferred composition as holding garrisons when nothing else is worth buying. Without a fleet sink, D-083 item 2 fails once the sites are taken, D-070's fleet target has no fleet to measure, and raids meet nothing. A4 replaces it.

**✅ Q214** → D-213: **What Expand optimises:** Payback within the time left in the season (kit price, productive build-out and logistics against the yield), plus a control term (towards a majority, completing one, denying a rival's), minus risk (route time, haven distance, sensed hostiles, rival holdings), crystal first while crystal income is zero, scaled by personality.

**✅ Q215** → D-214: **Plans and the online session:** The Strategist knows its schedule (A1 Q205) and starts a plan only if it ends inside the session; otherwise it pre-positions the MULE at its nearest holding. At the last evaluation before the session ends it signs off: Balanced doctrine with every fleet given a role, the replacement list, queues filled for the income until the next session, convoy routes set.

**✅ Q216** → D-215: **Built-in blueprints:** One blueprint per site kind (mother, ore, two-ore, crystal, ore and crystal, empty) as rules data: productive modules first, then storage where needed, pylons, two turrets, and a dry dock only at the mother station and one forward holding. The blueprint type goes into `nomad-types` for U3. Easy uses a generic order; Hard adds turrets where raids strike.

**✅ Q217** → D-216: **Depots or convoys:** The AI builds storage at its first holding in every system and runs convoys only until it completes. The α runs measure how much income still travels by convoy; if convoys have all but vanished, raise the storage module's cost (a number, D-137) so convoys keep the role D-064 gave them.

**✅ Q218** → D-217: **How late income is bounded:** A diminishing yield. Each producing holding other than the mother station has its alloy and crystal output scaled by min(1, F ÷ the commander's producing holdings other than the mother station), with F a rules value starting at 2 (about 3.6× late in the model) and tuned at α, and the factor shown in the economy view. A kit price in any form only delays site exhaustion (§5.3), and the 8-week season outlasts any delay that keeps the early targets. This is the option the owner passed over for D-189; the model shows D-189 cannot bound late income alone. D-189 stays, keeping early expansion paced. A rule change to doc 14; D-189 stays.

**✅ Q219** → D-218: **How α measures pacing with real AI:** `nomad-harness --season` with 4 AI commanders (A1–A3, Normal, seeded schedules), 3 seeds over the full 8 weeks, plus 7-day runs on 10 seeds for tuning; D-070's six measures with the definitions in §5.4 (first expansion from the first online minute; mid-season days 21–35; late days 42–56); S4's scripted builder on the same seeds alongside; pass when each median is within ±50%. Settles D-070's "first expansion within 1 h" for scheduled AI (D-080).

**✅ Q220** → D-219: **Which parts of the raider director are rules and which are AI:** The launch is a simulation rule: a timer, eligibility, a target drawn by weights from public data, strength and composition from the target's real fleet value, and the spawn. The director AI only controls raid groups, through the NPC view and ordinary commands. Reconciles D-081 with D-077: launch, target and strength are a simulation rule, not an AI decision.

**✅ Q221** → D-220: **Fleet value, and a floor:** Build cost as alloy + 3 × crystal (D-066's ratio), docked ships included; NPC classes valued as rules values (raider strike craft 1,600, HSTL 17,000 to start); a floor of 2 strike craft, so undefended convoys and outposts are raided too.

**✅ Q222** → D-221: **Where raid groups come from, and where they go:** Each launch spawns a fresh group at the haven's gate, and the group is removed when it returns. There is no raider economy, and survivors do not accumulate.

**✅ Q223** → D-222: **When the 10 minutes start:** At arrival in the target system, when the shroud ends. Losses count by value against the spawned group. Withdrawal is a jump back to the haven, earlier if the sensed hostile value within 5 km exceeds twice the group's.

**✅ Q224** → D-223: **Raiders and peripheral structures inside the owner's window:** Raiders only disable peripheral structures, at any time. That matches D-081's wording and its intent (raids pressure; they do not wipe out). S6b's `structure_outcome_at_zero` takes the attacker's owner into account. Settles D-081 and doc 13 §8 against doc 15 §10, doc 12 §3 and D-169 for NPC raiders.

**✅ Q225** → D-224: **An outpost's hull while it is built:** A core under construction has a hull that grows linearly from 10% to 100% of its maximum over the 2 h deployment. A fresh claim is fragile and a nearly finished one is not, which rewards escorting it early.

**✅ Q226** → D-225: **What "raids happen" means at α:** In every α season run: 1. each haven launches every 2–4 h, with every interval inside the bounds; 2. every commander is the target of at least one raid per 24 h on average, and never twice within 6 h; 3. at least half of the raids that reach their system destroy or disable something; 4. no dome ever takes raider damage; 5. no raid launches during catch-up (one run includes a catch-up segment).

## W. Round 18, from `21-browser-client-and-local-container.md` (answered)

The owner's direction is agreed as D-235 (the browser is the player's client), D-236 (the server in Docker Desktop, one player, no internet, no login) and D-237 (single player first, then MP-1 with up to 4 humans). The questions below are how to carry it out; the full text, with recommendations and alternatives, is in doc 21 §11. Q227–Q229 were not used.

- **✅ Q230** (D-238) — The client's technology.
- **✅ Q231** (D-239) — The language of the HTML UI.
- **✅ Q232** (D-240) — Where the phase-1 HUD (M6) is drawn.
- **✅ Q233** (D-241) — The transport.
- **✅ Q234** (D-242) — The QUIC endpoint.
- **✅ Q235** (D-243) — Serving the client.
- **✅ Q236** (D-244) — The guard without a login.
- **✅ Q237** (D-245) — The process model in Docker Desktop.
- **✅ Q238** (D-246) — Fonts.
- **✅ Q239** (D-247) — Browsers.
- **✅ Q240** (D-248) — Keys and fullscreen in the browser.
- **✅ Q241** (D-249) — Notifications.
- **✅ Q242** (D-250) — Build, CI and tests.
- **✅ Q243** (D-251) — Where MP-1 sits.

## X. Round 19, from the β boards, the C7 inventory and the station kit (answered)

Q244–Q254 come from the view-model inventory (`plans/C7-view-model-inventory.md`, which has their full text), Q255 from the station-kit art pass, Q256–Q265 from the β client UI boards (`plans/U-beta-ui.md`), and Q266–Q267 place S9 and K1. All were answered as recommended (Q255 in its capped-sector form) except Q266, where S9 goes after α.

**✅ Q255** → D-255: **Pylons outside the dome.** Should pylons sit outside the shield, where attackers can reach them, so that defending the rim matters? *Recommendation:* yes, on the peripheral rule (disabled outside the owner's window, destroyable inside), with a lost pylon capping its sector at 25% (D-140) and raiders not targeting pylons. *Alternatives:* a destroyed pylon zeroes its sector and breaches it (bypasses the dome's strength and, outside the window, the stakes rule); pylons stay inside and untargetable (doc 15 §5.1 as written).

- **✅ Q244** (D-264): Names of systems, sites, sectors and commanders.
- **✅ Q245** (D-265): How the client gets the rules.
- **✅ Q246** (D-266): How the topology reaches the client.
- **✅ Q247** (D-267): Fleet names.
- **✅ Q248** (D-268): Per-holding output.
- **✅ Q249** (D-269): The pool's fill estimate.
- **✅ Q250** (D-270): Route preview.
- **✅ Q251** (D-271): Alert lifecycle.
- **✅ Q252** (D-272): The start screen's API.
- **✅ Q253** (D-273): Reordering the build queue.
- **✅ Q254** (D-274): STOP on the fleet card.

**✅ Q256** → D-256: **The β HUD's right-hand panel.** The question: what replaces phase-1's Objectives and Commanders, which have no MVP data. *Recommendation:* a holding panel: window, dome sectors, Reinforced and siege timers, Deputy deferred decisions. *Alternatives:* keep an objectives panel (needs an objective design); drop the panel.

**✅ Q257** → D-257: **NETWORK in the top bar.** The question: commanders online and ping mean nothing for one player on localhost. *Recommendation:* server status (live or catching up, tick rate, D-245's clock check) plus the Deputy chip. *Alternatives:* keep the network readout for MP-1.

**✅ Q258** → D-258: **ELEV REF on the positional card.** The question: D-093's 12-button grid has no room for ELEV REF beside BUILD. *Recommendation:* ELEV REF becomes a camera-widget toggle; BUILD takes R. *Alternatives:* keep ELEV REF on the card and move another button to a submenu.

**✅ Q259** → D-259: **Build buttons when the pool is short.** The question: D-068 lets an unaffordable item queue and wait. *Recommendation:* enabled, showing the shortfall. *Alternatives:* disabled until affordable.

**✅ Q260** → D-260: **Switching to a strategic view.** The question: the scene is wgpu and the strategic views are SVG. *Recommendation:* a cut, with the canvas paused. *Alternatives:* a short cross-fade.

**✅ Q261** → D-261: **F4 until δ.** The question: with one sector a minimal GALAXY map repeats SECTOR. *Recommendation:* a full-screen season view. *Alternatives:* a minimal galaxy map as doc 17 §3 has it.

**✅ Q262** → D-262: **The cancel key.** The question: the browser reserves Esc in fullscreen. *Recommendation:* right-click on empty space or Backspace; Esc in a window. *Alternatives:* leave it to U7.

**✅ Q263** → D-263: **Where the start screen sits.** The question: doc 30 has it in U6 (γ), but β needs it (D-245). *Recommendation:* a minimal start screen in β; U6 completes it in γ. *Alternatives:* keep it in γ and create Universes from the command line in β.

**✅ Q264** → D-275: **The HUD below 1600 px.** The question: D-247's 1280×720 minimum with uniform scaling gives about 8 px text. *Recommendation:* a compact layout, text at 11 px or more. *Alternatives:* raise the minimum to 1600×900; keep uniform scaling.

**✅ Q265** → D-276: **Who draws the strategic views.** The question: doc 21 keeps the scene in wgpu and the UI in HTML, but does not say where SYSTEM and SECTOR belong. *Recommendation:* HTML/SVG. *Alternatives:* wgpu, which allows a continuous zoom from TACTICAL; mock both first.

**✅ Q266** → D-277: **Where S9 goes.** The question: S9 changes hit tests and station obstacles that A1b and A3 build on. *Recommendation:* now, beside A1a. *Alternatives:* after α (chosen); split into the template now and the pylon rules after α.

**✅ Q267** → D-278: **Where K1 goes.** *Recommendation:* after S9 and before C8, on fixtures first. *Alternatives:* right now on fixtures, with rework if S9 moves positions; fold it into M5.

## Y. Round 20, from maintaining the voxel art in MagicaVoxel (answered)

**✅ Q268** → D-289: **Who owns the voxel art.** The owner wants to maintain the models in MagicaVoxel. Until now the JSON files were the truth, and two generators rewrote them: `tools/station-spread` rebuilds the phase-1 diorama and `tools/station-kit` generated the module-kind models in code, so edits made in an editor would be overwritten. *Recommendation:* `.vox` files are the source of truth and the JSON is generated from them by a Rust tool in the workspace, with a test that fails when the two disagree; module kinds and ships first. *Alternatives:* JSON stays the truth with a throwaway round-trip through `.vox`; a hybrid where only hand art is `.vox`; the phase-1 diorama as well (left out: D-278 retires it); a Python converter (no CI coverage). Owner chose the recommendation, module kinds and ships.
