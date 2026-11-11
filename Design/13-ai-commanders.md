# 13 — AI commanders, the Deputy and raiders

Status: **Agreed (round 9).** Q71–Q77 were decided as D-077 to D-083. Built on R-05, P-02, D-013, D-016, D-020, D-027, D-032, D-038, D-047, D-049, D-057 and D-075/D-076, and docs 11, 12, 14, 15 and 16. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Scope

This document defines:

- how the AI stack is structured;
- the Deputy's concrete behaviour, which implements doc 12 §5;
- the Strategist that plays an AI commander;
- AI commanders' online and offline schedules;
- difficulty;
- the NPC raider director;
- doctrine presets;
- AI acceptance criteria.

It is the largest risk in the MVP (RK-03), so the design favours what can be built in stages and tested headlessly (D-037).

## 2. Constraints already agreed

*Derived:*

- **Same rules, same information** (D-027). AI code reads only a `CommanderView` (doc 16 §7) and **acts only through the same commands a human client sends**. No resource bonuses, no map hacks.
- **Deterministic:** the AI runs inside the simulation step, with seeded per-commander streams (doc 11 §4.1, D-049). Its decisions are never journaled; replaying the journal reproduces them.
- **Evaluation rates:** the Deputy runs at 2 Hz per holding and fleet, staggered. The Strategist runs every 10 s per commander, staggered (doc 11 §4.1). *(10 s is Hard's interval; Normal's is 15 s, §7, D-229. Phases and budgets: D-203.)*
- **The Deputy is always on, and its authority is limited** (D-013, D-016). Its tactical competence is capped below an engaged human (doc 12 §5.4).
- **The MVP is a free-for-all with no diplomacy** (D-026, D-027). Stage 1 has 3 AI commanders plus raiders (D-038).
- **Raiders never breach** (D-056). No new raids start during catch-up (D-047).

## 3. Architecture

*Agreed (D-077); this adopts P-02.*

```text
           ┌──────────────── CommanderView (doc 16 §7) ─────────────────┐
           │                                                            │
 Human ──► commands ─┐                                                  │
                     ├──► simulation step (doc 11) ◄── commands ── Strategist (AI commanders only)
 Deputy (everyone) ──┘                                     writes its Deputy's doctrine
 Raider director (NPC) ──► commands ───┘
```

| Layer | Who has it | Runs | Does |
|---|---|---|---|
| **Deputy** | Every commander, human or AI | Always, at 2 Hz | Executes doctrine within D-016: defence, abilities, power profile, evacuation, rebuilding from the replacement list, convoy routes |
| **Strategist** | AI commanders | Only while that AI is "online" (§6), every 10 s | Everything a human decides: expansion, build orders, fleet composition, raids, breaches and sieges, scouting. Writes its own Deputy's doctrine before going "offline". |
| **Raider director** | NPC raider factions | Always, except that no new raids start during catch-up | Launches and controls raid groups (§8) |

> **Settled with the A1 plan (D-199 to D-203):** all AI logic lives in a `nomad-ai` crate that reads the Universe only through a `View` trait over `CommanderView`, so it cannot name `UniverseState` (D-199). AI commands go through a per-owner outbox into phase 1 of a later tick (D-200), within the competence cap of D-201; events reach the AI through a per-owner inbox (D-202); D-203 places each layer in the tick phases and sets its budget. D-219 makes a raid's launch, target and strength a simulation rule, so the raider director only steers the raid groups (§8).

**One command interface.** The Strategist and the raider director issue the same command types as a human client (doc 15 §3.4 orders, plus build, claim and doctrine commands). This makes fairness checkable, and it means one test suite covers every source of commands.

## 4. The Deputy

*Agreed (D-206), with the details of `plans/A1-deputy.md` (D-199 to D-211). These are the concrete rules behind doc 12 §5 (D-016).*

1. **Threat assessment** for each holding, every 0.5 s:
   - A holding is on *alert* when a hostile in the commander's visibility set is inside the defended zone (the doctrine's radius around the site), or the commander learns of hostile fire on the holding, its modules or dome, or on an own ship inside the zone.
   - Alert ends 60 s after the last such event. Fire on an asset outside every zone puts no holding on alert; that fleet's own doctrine answers it (return fire, retreat, convoy evacuation).
   - `AlertStarted` and `AlertEnded` go to the away report.
2. **On alert:**
   - Switch to the doctrine's alert power profile (doc 15 §5.2); at alert end, back to the profile before.
   - Launch the garrison, fleet by fleet; reserves and convoys stay docked.
   - Use abilities on their triggers (SHLD SURGE and FLAK SCREEN thresholds, §9), when power and the cooldown allow.
   - Dock convoys within the evacuation radius (below). The MVP has no harvesting craft: harvesters are modules (doc 14 §6).
3. **Engagement:**
   - Target priority, as explicit attacks issued only when the choice changes: first units that damaged our structures in the last 30 s, then targets the counter triangle favours (D-052), then the nearest, then the lowest `SimId`.
   - Never pursue beyond the leash, measured from the fleet's anchor: the holding for a garrison, the route for a patrol, the escortee for an escort. Retreat to the nearest own holding with room when the fleet's hull falls below the retreat threshold.
   - Roles (D-207): a garrison docks at its holding, launches on alert and docks when the alert ends; a reserve stays docked; a patrol runs its route; an escort guards its fleet. A damaged fleet with no alert repairs at dock (D-071).
4. **Rebuilding** (D-208): when a ship from the replacement list is lost, queue a replacement at the nearest holding able to build it, and rebuild destroyed modules in their slot, within the preset's share of the last 24 h of income; report what is deferred.
5. **Logistics** (D-064, D-210):
   - Run each convoy route.
   - When a sensed hostile is within the evacuation radius of a convoy or its next stop, dock it at the nearest own holding on its route, and resume the route 60 s after no hostile is within the radius.
   - If a gate on the route is contested (D-055, by hostiles the commander detects, D-285), reroute when another route exists; otherwise hold at the last safe stop.
6. **Competence cap** (doc 12 §5.4):
   - A decision takes effect at least 3 ticks (300 ms) after the event that triggered it.
   - A commander's AI layers together issue at most the difficulty's command rate (§7).

## 5. The Strategist

> **Settled with the A2 plan (D-212 to D-218):** goals Expand, Build out, Logistics and a Garrison stand-in until A4 (D-212); Expand weighs payback within the season's time left, control and risk, crystal first (D-213); plans fit the online session (D-214); built-in blueprints per site kind (D-215); a depot in every system (D-216). D-078's "every 10 s" is Hard's interval (D-229). *Built with A2* (`crates/nomad-ai/src/strategist/`; readings in the A2 plan §12): the four A2 goals score on one integer scale, keep at most three plans, reserve their next costs in score order and sign off before the session ends; a loaded kit sets out whatever the session's length, since its trip continues offline (D-291); the A4 goals below are not built yet.

*Agreed (D-078).*

**Utility-scored goals.** Every 10 s, the Strategist scores a fixed set of goals from features of its `CommanderView`. It keeps a plan for each of the best few, as long as resources and fleets allow.

| Goal | When its score rises | Plan, as ordinary commands |
|---|---|---|
| **Expand** | Free sites with good deposit value nearby; the command cap and the pool allow it | Build a core kit, send a MULE, deploy, escort |
| **Build out** | Empty slots at a holding; storage is filling | Follow the holding's blueprint build order (D-067) |
| **Logistics** | Remote buffers are filling (D-064) | Build a depot, or assign MULE convoys |
| **Compose fleet** | The current mix is weak against the hostiles it has seen (counter triangle) | Queue counter units |
| **Defend** | Threats near its holdings, or an incoming siege announcement | Mass fleets at the threatened holding for the siege start |
| **Raid** | Peripheral enemy assets that are visible and poorly defended | Send a strike group to raid, then withdraw |
| **Breach and siege** | An enemy holding it can take, given its own strength and the target's defences | Breach any time. Then **plan to be online and present at the siege start**, which lies in the defender's window (D-015, D-057). |
| **Scout and picket** | Neighbouring gates without coverage (D-074) | Station a cheap picket at those gates |
| **Retake** | It recently lost a holding and could win it back | A breach-and-siege plan against that holding |

- **Siege timing rule:** the Strategist only commits to a siege whose start falls inside **its own** online time (§6), exactly the constraint a human attacker faces (D-057).
- **Personality** (seeded per AI commander): *aggression*, *appetite for expansion*, *risk tolerance* and a *preferred composition*. Each scales the goal scores, so nine AI commanders do not play identically.
- **No identity bias:** target scoring never looks at whether a commander is human or AI. The human is neither focused nor spared.

## 6. AI online and offline schedules

> **Settled with the A1 plan (D-204):** D-080's seeded schedules run in the simulation from A2 on, with the harness `SetPresence` kept as an override. *Built with A2* (`crates/nomad-ai/src/schedule.rs`): a weekday and a weekend pattern; the server creates its AI commanders on them, the harness and tests may still drive presence with `SetPresence`.

*Agreed (D-080), detailing D-027.*

- Each AI commander gets a seeded weekly schedule: **2–3 sessions a day, 1.5–4 h in total, at consistent times**, with some variation at weekends.
- The connect and disconnect events come from the seed and are not journaled (doc 11 §6). They feed the activity-derived exposure window (D-032), so an AI's window reflects its schedule, just as a human's does.
- **While online,** the Strategist plays. **Before going offline,** it writes its Deputy's doctrine (§9). **While offline,** only the Deputy runs. The human therefore meets Deputy-run empires, and the AI meets the human's Deputy.
- **During catch-up** (D-047), every AI commander is offline, just as every human is.
- Headless test runs may override schedules, for example "always online", as a harness option. That is not time compression (D-036).

## 7. Difficulty

*Agreed (D-079).* Difficulty changes **competence only**, never resources or information (D-027).

| Setting | Easy | Normal | Hard |
|---|---|---|---|
| Reaction latency | 1.0 s | 0.6 s | 0.3 s (doc 12 §5.4) |
| Command rate (all AI layers of one commander) | 1/s | 2/s | 4/s |
| Strategist evaluation interval | 30 s | 15 s | 10 s |
| Decision noise (seeded, applied to goal scores) | High | Medium | Low |
| Build-order quality | Generic | Blueprint per site kind | Blueprint per site kind, adapted to threats |
| Scouting and pickets | No | Yes | Yes, and it reacts to lane traffic |

The difficulty is set per AI commander when the Universe is created. The MVP default is Normal.

> **D-201:** latency is counted in ticks from the triggering event (10, 6 or 3; 3 for a human's Deputy), and the command rate holds in any 10 consecutive ticks (4 for a human's Deputy). AI parameters live in the rules file's `ai` section (D-211). As built, `nomad-server create --difficulty` sets one difficulty for every AI commander.

## 8. NPC raider director

> **Settled with the A3 plan (D-219 to D-225):** a raid's launch, target and strength are a simulation rule; the director AI only steers spawned raid groups through the NPC view (D-219, reconciling D-081 with D-077). Fleet value is alloy + 3 × crystal cost, with at least 2 strike craft (D-220). Groups are spawned per raid and removed on return (D-221); the raid clock starts on arrival (D-222). *Derived* (D-313, superseding D-220's floor and the 10 minutes of D-081 and D-222): at least 4 strike craft, and a 15-minute clock. Raiders only ever disable structures (D-223). An outpost's hull grows from 10% to 100% while it is built (D-224). D-225 defines "raids happen" for α.

*Agreed (D-081); this is the behaviour behind doc 15 §10.*

- **Timing:** each raider haven launches a raid every **2–4 h**, the interval drawn from its seeded stream. No new raids start during catch-up (D-047).
- **Target choice:**
  - Weighted by asset value and by distance from the haven.
  - Eligible targets: convoys, ships outside docks, outposts under construction (D-054), and peripheral structures, which are disabled only (doc 12 §3).
  - **No commander is raided twice within 6 h.** Online status plays no part (it is private, D-075).
- **Strength:** 30–50% of the target commander's fleet value in that system. Raids pressure a commander; they do not wipe them out.
- **Withdrawal:** after 10 min, or after losing 50% of the group. *(Derived from D-313: the clock is 15 min, and every raid has at least 4 strike craft, so that at least half of the raids that reach their system destroy or disable something, D-225 item 3.)*
- Raiders do zero damage to domes (D-056). They see through their faction's own sensors (doc 16 §7).

> **As built (A3):** the launch rule is in `nomad-sim` (`raids`): one NPC faction per haven, a launch timer drawing 2–4 h on the haven's `RAIDS` stream, a target drawn by public-data weights, strength from the target's fleet value in that system, the spawn at the haven's gate, and removal on return; a firing during catch-up launches nothing and is not deferred (D-047, D-219 to D-221). The director is in `nomad-ai` (`director`): it steers each group through the faction's view and ordinary commands at Normal's competence, starts the raid clock (*Derived:* 15 minutes, D-313) when the arrival shroud ends, attacks the target commander's MULEs, outposts, weaker ships and peripheral structures in that order, and withdraws on the clock, at half its value lost or before a force more than twice its own (D-222). D-223 and D-224 were already built by S4 and S6b. The readings the plan left open, what the first `--raids` runs showed, and the D-313 confirmation in AI play are in the A3 plan §12.

## 9. Doctrine presets

*Agreed (D-082).* These are the presets from doc 12 §5.2. *Balanced* is the default for new human commanders, and the Strategist's starting point before it adapts its doctrine.

| Parameter | Fortress | Balanced | Trader |
|---|---|---|---|
| Defended zone radius | 15 km | 10 km | 8 km |
| Rules of engagement in the zone | Weapons free | Weapons free | Return fire |
| Alert power profile | Shields first | Shields first | Balanced |
| SHLD SURGE when a sector falls below | 40% | 30% | 20% |
| FLAK SCREEN when light hostiles within 3 km reach | 6 | 8 | Never |
| Evacuate harvesting craft and hold convoys when hostiles are within | 30 km | 20 km | 10 km |
| Fleet retreat threshold | 50% hull | 40% | 30% |
| Leash | 8 km | 12 km | 6 km |
| Replacement budget (share of daily income) | 50% | 30% | 15% |
| Share of the fleet kept as garrison | 70% | 50% | 30% |

## 10. AI acceptance criteria

*Agreed (D-083); input for doc 30.* These are measured in headless AI-against-AI runs (D-037), then confirmed by human playtests.

1. **Determinism:** the same seed gives the same state hash with AI commanders active (D-049).
2. **No idle income:** over any 24 h, each AI spends at least 90% of its income, or is at the cap with nothing left to build.
3. **Expansion:** unopposed in MVP stage 1, a Normal AI holds 4 or more holdings by day 3 (doc 14 pacing target: one new holding per 12–24 h).
4. **Combat:** against an equal-value force, a Normal AI using the counter triangle wins at least 60% of fleet battles, compared with a baseline AI that does not use it.
5. **Sieges happen:** in a 14-day four-commander run, at least 3 sieges start and at least 1 holding changes hands. The point is to catch stagnation (RK-02).
6. **Nothing gets stuck:** no unit is idle while ordered, or unable to make progress, for more than 10 min.
7. **Budget:** mean Strategist evaluation of 5 ms or less per commander; Deputy work within the tick budget (doc 11 §4.3).
8. **Fairness:** a build-time test guarantees that AI code cannot reach `UniverseState` (doc 16 §7). A runtime check verifies that every AI command passes the same validation as a human command.

## 11. Build order

*Proposed; input for doc 30.* *(Decided as D-205: A1a after S6a, A1b after S7 and S5b, A2 after A1a and S7, A3 after S6b, S7 and A1a, in parallel with A2; doc 30 §4.)*

1. **Deputy:** defence, abilities, retreat, rebuilding, convoys. Every commander needs it, the human included.
2. **Strategist economy:** expand, build out, logistics. This gives a "builder AI" to test economy pacing with (D-070).
3. **Raider director.** It gives early combat pressure without the full Strategist.
4. **Strategist military:** compose, defend, raid, breach and siege, scout.
5. **Personalities and difficulty.**

Each step is measured in the harness against §10 before the next one starts.

## 12. Stress test

1. **A fair AI may be too weak for a fun MVP.** Mitigations:
   - the free-for-all means AI commanders also fight each other;
   - raiders add pressure;
   - Hard difficulty runs close to the human cap;
   - personalities make AI commanders less predictable.

   If playtests still find the AI weak, the honest levers are better behaviour or easier settings for the human, not cheats (D-027).
2. **Siege timing is the hardest reasoning step.** The rule in §5 reduces it to one question: will I be online and in position when the siege starts? It does not require planning across the whole calendar.
3. **A 10 s Strategist reacts slowly** to a surprise attack. The Deputy covers the first minutes at 2 Hz, the same split a human experiences.
4. **Utility AI can oscillate** between goals. Plans stay committed for a minimum time, and switching to another goal costs score (hysteresis).
5. **Predictability:** humans learn AI patterns. Seeded personalities and decision noise, plus the D-037 harness for spotting degenerate strategies, keep this in check.
6. **Scope:** this is still the largest item in the MVP. §11 stages it so that every step produces something testable, matching RK-03 and D-038.

## 13. Effect on other documents

- **Doc 12:** the Deputy rules in §4 and the presets in §9 make §5 concrete.
- **Doc 17:** the doctrine editor exposes the §9 parameters and presets.
- **Doc 20:** AI modules sit inside `nomad-sim`, behind `CommanderView`, with a single command validator. *(Amended by D-199: the AI is the separate `nomad-ai` crate, behind a `View` trait that `nomad-sim` implements over `CommanderView`; the validator stays single.)*
- **Doc 30:** §10 and §11 become milestones and acceptance criteria.
