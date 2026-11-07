# Work package A2 — The Strategist: economy

Status: **Planned and decided** (Q213–Q219 → D-212 to D-218, and Q205 in `A1-deputy.md` → D-204, which moves AI schedules into A2). Written together with `A1-deputy.md`, whose §4 holds the shared architecture A2 runs on, and `A3-raider-director.md`. Builds on A1a (the `nomad-ai` crate, outbox, competence gate, doctrine), S4 (economy, built; the D-217 diminishing yield built with it), S6a (presence, grace, lapse; built) and S7 (`CommanderView`); reads S8's standings (built). This plan is also the brief for whoever builds A2. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval. *Build status (2026-11-07):* Not built yet; brief in `briefs/A2.md`.

## 1. Goal

AI commanders play the economy the way a human does:

- they expand to new sites;
- they build their holdings out;
- they keep logistics running;
- they spend what they earn;
- they write their Deputy's doctrine before going offline.

They act only while online by their seeded schedule (D-080), see only their `CommanderView`, and act only through ordinary commands (D-077). They score utility goals every evaluation interval (D-078, D-079). With A1 and A3 this is the α "builder AI": it replaces S4's scripted builders as the measure of the D-070 pacing targets, which α needs within ±50% (doc 30 §3). It also meets D-083's economic criteria: no idle income, and 4 holdings by day 3 when unopposed.

## 2. Inputs

- **The Strategist:**
  - doc 13 §5 and D-078: utility-scored goals (expand, build out, logistics, …), plans of ordinary commands, seeded personalities, no identity bias;
  - doc 13 §12 item 4: minimum commitment and switching costs against oscillation;
  - D-079: difficulty changes competence only (Normal: 15 s interval, medium noise, blueprints per site kind);
  - D-080: schedules;
  - D-082: Balanced is the Strategist's starting doctrine;
  - D-083 items 2, 3, 6, 7: no idle income, expansion, nothing stuck, budget.
- **Economy:**
  - doc 14 and the S4 plan; D-064 (pool, depots, buffers, convoys), D-068 (queues: paid at start, waiting items do not block), D-070 (pacing targets), D-112 (start kit), D-129 (command cap), D-130/D-131 (slots), D-133/D-134 (MULE cargo, convoy routes), D-136 (build-out read against the holding's own output), D-137 (measure first, then tune numbers);
  - D-189: a core kit costs base × (1 + k × n), k = 0.25, tuned at α.
  - S4's 7-day run: income 7.8× start against the 3–4× target, the fastest scripted builder at 15 holdings by day 7 (commit `11eac17`).
- **Initiative and presence:**
  - PR-3: expanding and claiming are the commander's own initiative, so the Deputy never does them;
  - D-160 to D-162: direct orders lapse 120 s after going offline;
  - D-157: an AI's initial window comes from its schedule.
- **Universe:** D-041 and D-020 (control decides the season); D-043 (about 15 sites per commander); D-066 and D-098 (deposits); doc 10 §3 ("by late season roughly half the sites are claimed").
- **Code:**
  - `crates/nomad-sim`: `CommanderEconomy` (pool, cap, income over the last hour, command, the next kit's price), `HoldingEconomyView`, `QueueItemView`, economy commands including `CoreKit`, `LoadCoreKit` and deploy;
  - the rules file's `economy` section (queue limit 24 items, MULE 2,000 units, 8 convoy stops);
  - `crates/nomad-harness/src/economy.rs`: the scripted builders and the D-070 report.

## 3. Scope

**In scope:**

- **The Strategist framework:** evaluation on schedule, features from the view, goal scoring with personality weights, noise and hysteresis, plans as step machines, resource reservation, plan timeouts.
- **Goals:** *Expand*, *Build out* and *Logistics* (doc 13 §5), plus a stand-in *Garrison* goal until A4 (Q213).
- **Built-in blueprints** per site kind (Q216).
- **Sessions:** plans that fit the online time, and the sign-off before going offline, with doctrine and queues for the offline period (Q215).
- **Seeded AI schedules** moved forward from A5 (A1 Q205).
- **The difficulty table's plumbing,** at Normal for every AI.
- **Pacing with real AI:** the harness measures and the analysis of late income (§5, Q218, Q219).

**Out of scope:**

- *Compose fleet*, *Defend*, *Raid*, *Breach and siege*, *Scout and picket*, *Retake* (A4).
- Seeded personalities (A5; A2 runs neutral weights), per-AI difficulty selection (A5), and schedule tuning (A5).
- Player blueprints, saving and applying them (U3; A2 provides the data type).
- The AI against the human in playtests (γ).

## 4. Design

### 4.1 The Strategist loop

*Agreed* model (D-078, D-079); mechanics *Proposed*.

- **When:** in phase 3 (A1 §4.5), while its commander is online, every 15 s at Normal (D-079), staggered by commander. Also once at the sign-off before going offline (§4.8). Never during catch-up (D-080).
- **Each evaluation:**
  1. reads features from its view (§4.2);
  2. scores every goal as an integer utility, times the personality weight (neutral at A2), plus seeded decision noise (Normal: medium), plus a hysteresis bonus for goals with an active plan;
  3. keeps plans for the best three goals, reserving each plan's next costs from the pool in score order, so a lower plan never spends what a higher plan needs;
  4. advances each plan by at most one step, sending commands to the outbox (A1 §4.3);
  5. checks progress against the inbox: a plan step that has not progressed for 10 min is abandoned, and the fleets are released (D-083 item 6, RK-10).
- **Hysteresis** (doc 13 §12 item 4): a plan, once started, is kept for at least 5 min, and its goal scores +20% while it runs. Both are rules values (A1 Q212).
- **Fleets:** the Strategist's orders put fleets under direct control (A1 §5.2). When a plan ends, the Strategist releases them to doctrine.
- **Determinism:** its own random stream, integer scores, `SimId` tie-breaks (A1 §4.6).

### 4.2 Features from the view

*Derived:* everything comes from `CommanderView` (D-077), by information class (S7 plan §4.7).

- **Own:**
  - pool, cap, income over the last hour, command used and cap, the next kit's price (D-189);
  - per holding: slots, queue, power, buffer, depot status;
  - fleets, with docked MULEs and cargo;
  - doctrine;
  - the commander's own schedule (§4.8).
- **Public:**
  - sites with deposits and richness, ownership including outposts, control states, standings and the season clock (S8);
  - route times between systems;
  - every commander's window and `breachable_from` (S6);
  - the raider haven's position in the topology.
- **Sensed and last-known:** hostile fleets per system, by position and composition (S7).
- **Lane traffic:** departures towards its systems (D-074, D-179).
- **Inbox:** losses, raids, deferred decisions, command outcomes (A1 §4.4).
- **Caches** in memory: the season's topology is fixed (D-044), so route times between systems and sites are computed once and kept.

### 4.3 Goals at A2

*Agreed* goals (doc 13 §5); the stand-in is *Proposed* (Q213).

| Goal | Score rises with | Plan |
|---|---|---|
| **Expand** | A free site whose holding pays back within the time left, adds control, and is safe enough to claim (§4.4) | Queue a core kit, ensure a MULE, load, travel, deploy, guard the outpost if at risk |
| **Build out** | Empty slots with productive or defensive value; a next kit priced above the pool cap | The holding's blueprint, next affordable step (§4.5) |
| **Logistics** | A holding with no depot in its system and a filling buffer | Storage at the first holding in the system; otherwise a convoy (§4.6) |
| **Garrison** (stand-in until A4) | Nothing else worth buying, the pool above its reserve, the command cap not full | Ships of the preferred composition as holding garrisons (§4.7) |

### 4.4 Expand

*Agreed* goal (D-078); scoring *Proposed* (Q214).

- **Candidates:** every free, claimable site the commander can reach, at most 60 at stage 1 (S7's index keeps this cheap).
- **Value per hour:** the alloy plus 3 × crystal per hour (D-066's weight) its deposits would yield once productively built out. Diminishing yield applies if Q218 adopts it.
- **Cost:** the kit's price at queue time (D-189), the productive modules (solar where power needs it, harvesters, refineries, extractor) and its logistics (a storage module, or a share of a MULE).
- **Payback:** cost ÷ value per hour. A site whose payback exceeds the time left in the season (S8's season clock), or the personality's horizon, gets no income term.
- **Control term:**
  - a bonus when the site moves the commander towards a strict majority in that system (D-041), counting its holdings and outposts there;
  - a larger bonus when it completes a majority;
  - a denial bonus when a rival is one site short of a majority there.
  
  Control decides the season (D-020), and most MVP seasons will be decided by tie-breaks on holdings (S8 plan §9 item 1), so empty sites have value too.
- **Crystal first:** while crystal income is zero, a crystal vein outranks everything. Without it the second kit never comes (S4 plan §9 item 2).
- **Risk:** subtracts route time from the kit's holding (exposure in transit), jumps from the raider haven, sensed or remembered hostiles near the site or on the route, and rival holdings in the system.
- **Constraints:**
  - the kit's price fits the pool cap, or Build out first adds storage (D-189's *Derived* note);
  - a MULE exists, or one is built (1,500 A);
  - the plan fits the online time (§4.8).
- **Plan steps:**
  1. queue the kit at the own holding nearest the site with queue room;
  2. load it into a docked MULE (`LoadCoreKit`);
  3. `Launch`, `Jump` and `Move` to the site;
  4. deploy;
  5. if the risk is high, escort with spare garrison ships (`Guard`) until the outpost completes 2 h later (D-054), or until it is destroyed. If the session ends first, the sign-off makes the escort the outpost's garrison (A1 §5.6: it station-keeps by a core it cannot dock at), so the Deputy keeps it there after the lapse (D-162);
  6. release the fleets.
- **Pre-positioning:** if the trip will not finish within the session, the loaded MULE moves to the own holding nearest the site and docks, ready for the next session.

### 4.5 Build out and blueprints

*Agreed* (D-067: a blueprint is a slot assignment plus a build order; D-079: blueprints per site kind at Normal); the blueprints are *Proposed* (Q216).

- **Site kinds** by deposits: *mother*, *ore* (one field), *two-ore*, *crystal*, *ore and crystal*, *empty*.
- **Order, per blueprint** (rules data, A1 Q212):
  1. productive modules: solar where the core's 120 MW cannot run them (a harvester and refinery need 180 MW); harvester or extractor per deposit; a refinery per ore field;
  2. storage, if this is the first holding in its system (§4.6), or extra storage when the next kit exceeds the pool cap;
  3. pylons N, E, S, W (each sector from 25% to 100%, D-140);
  4. two turrets;
  5. a dry dock only at the mother station (built) and one forward holding;
  6. empty sites stop after storage and pylons.
- **Across holdings:** productive steps first, by value per cost. Defensive steps next, nearest the raider haven and the most-sensed hostiles first.
- **Easy** uses one generic order; **Hard** adds turrets where raids have struck (A5 tunes both).
- **The blueprint type** goes into `nomad-types`, so U3 can save and apply player blueprints with the same data (D-035).

### 4.6 Logistics: depots and convoys

*Agreed* model (D-064, D-134); the choice is *Proposed* (Q217).

- A storage module costs 1,500 A, as much as a MULE, needs no route, and makes its holding a depot, so the whole system delivers straight to the pool (D-064).
- **The Strategist therefore builds storage at its first holding in each system.** It runs a convoy only until that storage completes, or where no inner slot is free. A convoy is a MULE fleet whose route is a loop of remote holdings ending at the nearest depot (D-134, at most 8 stops).
- **The Deputy runs and protects convoys** (A1 §5.8).
- **Consequence** (§9 item 3): at these costs convoys almost disappear, and so does the raid target D-064 was meant to create. That is a tuning item for α.

### 4.7 The Garrison stand-in

*Proposed* (Q213); replaced by A4's *Compose fleet* and *Defend*.

- When no Expand or Build-out step is affordable and worthwhile, and the pool is above its reserve (the next planned kit), the Strategist queues ships up to the command cap in the preferred composition. That is neutral at A2: by value, about 30% VX, 40% LANCER and 30% WARDEN; WARDENs only where there is a dry dock.
- New ships join the building holding's garrison (A1 Q208), spread over holdings by exposure (nearest the haven, most hostiles sensed).
- The replacement list (A1 §5.7) is set to the current garrisons, so the Deputy keeps them up while the commander is offline.
- **Why:**
  - D-083 item 2 (≥ 90% of income spent, or at the cap with nothing left to build) cannot be met at α without a fleet sink once the sites are taken;
  - D-070's "full cap fleet in about 24 h" is only meaningful with fleets being built;
  - raids (A3) then meet resistance.

### 4.8 Sessions, sign-off and doctrine

*Agreed:* while online the Strategist plays; before going offline it writes its Deputy's doctrine; while offline only the Deputy runs (D-077, D-080); claiming is the commander's own initiative (PR-3); direct orders lapse after the grace (D-162). The mechanics are *Proposed* (Q215, A1 Q205).

- **Schedules** (A1 Q205):
  - generated in the simulation from the seed at Universe creation: 2–3 sessions a day, 1.5–4 h in total, at consistent times, with weekend variation (D-080);
  - stored in the commander's record, not journaled;
  - they drive S6a's presence and the initial window (D-157);
  - the harness's `SetPresence` overrides them.
- **Plans fit the session:** a plan starts only if its estimated duration (route time × 1.2, plus deploy) ends before the session does, else the MULE is pre-positioned (§4.4). The Strategist does nothing its human counterpart could not: a human must also be online when their MULE arrives.
- **Sign-off,** at the last evaluation that starts at least one interval before the session ends:
  - **doctrine:** Balanced (D-082) with every fleet given a role, uncommitted fleets as garrisons (A1 Q210), and the replacement list set to the garrisons;
  - **queues:** each holding's queue is filled with blueprint steps worth the expected income until the next session, within the 24-item limit, so the pool does not overflow while it is away (D-025, D-068: waiting items start as the money arrives);
  - **convoys:** routes set.
- **Offline:** only the Deputy runs. Its orders are doctrine, so nothing lapses except direct orders, and every fleet has been released.

### 4.9 Difficulty and personality hooks

*Agreed* (D-079); plumbing *Derived*.

- A2 reads the commander's difficulty from its record. The server's single `--difficulty` (N2) sets it for every AI. Per-AI difficulty is A5's.
- A2 implements Normal: a 15 s interval, medium noise and blueprints per site kind. The Easy and Hard rows are rules data, tested but tuned in A5.
- **Personality:** four weights (aggression, expansion appetite, risk tolerance, preferred composition), all neutral at A2. A5 seeds them from the commander's stream.

### 4.10 Budget

*Agreed:* a mean of 5 ms or less per evaluation (D-083 item 7); placement and measurement as in A1 §4.5.

- At stage 1, an evaluation scores at most 60 sites against at most 15 own holdings using cached route times, keeps at most 3 plans, and advances each by one step. Work counters bound every loop.
- The harness times evaluations directly through `nomad-ai` and reports the mean and p99 per commander.

## 5. Pacing with real AI: the late-income problem

*Derived* from the agreed rules, S4's measurement and a rough model; the remedy is *Proposed* (Q218, Q219).

### 5.1 What S4 showed

S4's scripted builders reached 7.8× starting income by day 7, against D-070's late-season 3–4×, even with D-189's kit price growth at k = 0.25. The reason is structural.

- **Each kit costs k × 6,000 A more than the last, and each holding adds about 600 A/h.** So a kit's price in hours of income levels off at about k × 10 h.
- **So the interval between expansions does not grow as the commander grows.** It tends towards k × 10 h plus the build-out's share of income: 17.4 h per holding over days 0–3 in S4's run, and shorter after that.
- **The sector holds about 15 sites per commander (D-043),** and they run out in two to three weeks.

### 5.2 How A2's behaviour differs from the scripted builders

These make the real AI slower than the scripted builders, which ran their script every minute around the clock.

1. **It plays only while online:** 1.5–4 h a day (D-080). Every expansion needs a session long enough for the trip (PR-3, D-162), so expansion happens in bursts.
2. **Crystal gates kits:** the home system's one vein (D-098) yields 60 C/h, and the n-th kit needs 500 × (1 + k × n) C. That alone spaces kits about 10–15 h apart until more veins are held.
3. **It values payback with the time left,** and weighs control against income (§4.4).
4. **It spends on defence:** pylons, turrets, the Garrison stand-in and the Deputy's replacements.
5. **Raids cost it:** destroyed outposts, lost MULEs and kits, disabled solar arrays (A3).

**But none of this bounds the total.** Each further holding still pays back within days. At k = 0.25 the 14th kit plus its productive build-out pays back in about 60 h, with weeks of season left. A rational player, human or AI, therefore keeps expanding until the sites run out. A2 must not stop short of that by design: an AI hobbled to meet a pacing number is a weak AI (doc 13 §12 item 1).

### 5.3 What tuning can and cannot do

A rough model of one stage-1 commander. It takes the cheapest-first order of the scripted builder (home crystal vein, home ore, neighbouring systems, two hub sites), the doc 14 costs and yields, and kit, travel and deployment times, and it builds extra storage when a kit would exceed the pool cap. It ignores sessions, raids and fleets. Its day-7 result for D-189 as built matches S4's measured 7.8×, which is the reason to trust its shape, not its decimals. The α harness replaces it.

| Kit price and yield | Day 3 | Day 7 | Day 14 | Day 28 | Day 56 |
|---|---|---|---|---|---|
| Linear, k = 0.25 (D-189 as built) | 3.2× · 5 | 7.8× · 10 | 14.2× · 15 | 14.2× · 15 | 14.2× · 15 |
| Linear, k = 0.5 | 2.5× · 4 | 5.8× · 8 | 9.8× · 13 | 14.2× · 15 | 14.2× · 15 |
| Linear, k = 1.0 | 1.8× · 4 | 4.8× · 6 | 6.8× · 9 | 14.2× · 15 | 14.2× · 15 |
| Geometric, base × 1.5ⁿ | 2.5× · 4 | 4.8× · 6 | 5.8× · 8 | 7.8× · 10 | 9.8× · 12 |
| Linear k = 0.25, half the sites each (doc 10 §3) | 3.2× · 5 | 5.8× · 8 | 5.8× · 8 | 5.8× · 8 | 5.8× · 8 |
| Linear k = 0.25, diminishing yield F = 3 (Q218) | 3.2× · 5 | 3.9× · 10 | 5.0× · 15 | 5.0× · 15 | 5.0× · 15 |
| Linear k = 0.25, diminishing yield F = 2 (Q218) | 2.5× · 5 | 2.9× · 10 | 3.6× · 15 | 3.6× · 15 | 3.6× · 15 |

Each cell gives alloy income as a multiple of start, then completed holdings including the mother station.

- **Any kit price only changes *when* the sites run out, not *whether*.** An 8-week season is long enough to claim every site at any price that still allows D-070's early pace (first holding within 1 h, one per 12–24 h early) and D-083's 4 holdings by day 3. Even doc 10 §3's picture of half the sites claimed gives about 6×.
- **The agreed targets, taken together, ask for alloy income to stop growing after about the fifth holding.** D-070's early pace and D-083's day-3 count already reach about 3× by day 3, and the late target is 3–4×.
- **Nothing in the agreed rules produces that.** With about 15 productive sites per commander (D-043, D-098) and about 600 A/h each, the season's ceiling is about 10–14× start. D-137's "only numbers are tuned" cannot close the gap, because the ratio is set by how many productive holdings there are, and the richness multipliers (D-066) and deposit counts (D-098) that set it are agreed.
- **A bound on yield works:** in the model, a holding's output scaled by min(1, F ÷ producing holdings beyond the mother station) holds income at about 5× with F = 3, and about 3.6× with F = 2. It still lets expansion continue for control and command cap, and leaves the early holding counts and intervals as they are. Total remote output becomes F × the average yield of the producing holdings, so rich sites, above all the hub's, become the prize (doc 14 §5), and a poor site is better held for control without a harvester. It also brings D-070's cap-fleet target into range: a 300-command fleet costs about 54,000 A, which at 3.6 × 600 A/h is about 25 h of income, against about 6 h at 14×.

### 5.4 What the α runs measure

*Proposed* (Q219).

- **`nomad-harness --season`** (A3 §5) runs 4 AI commanders with A1, A2 and A3 at Normal, on their seeded schedules, over the full 8-week season, with 3 seeds. 7-day runs on 10 seeds are for quick tuning.
- **D-070's six measures** per commander, median and range across commanders and seeds:
  1. storage fill time at start and over days 21–35;
  2. the first kit started, measured from the commander's first online minute: an AI whose first session starts at 07:30 cannot claim at 00:30, and neither could a human;
  3. hours per new holding over days 0–3;
  4. the cost of a full cap fleet in the Garrison composition ÷ mean income over days 21–35;
  5. D-136's build-out against the holding's own output;
  6. mean alloy income over days 42–56 ÷ 600 A/h.
- **Reported alongside:** S4's scripted builder on the same seeds, and holdings per commander per day.
- **α passes** when the median of each measure is within ±50% of its target (doc 30 §3).

## 6. Timeline check

*Derived* from the agreed values; AI behaviour as proposed here. A Normal AI on D-189 as built, with sessions 07:30–08:30, 12:00–12:45 and 19:00–20:30 UTC (3.25 h a day); season start at 00:00 UTC.

| Universe time | Event | Rule |
|---|---|---|
| Day 0 00:00–07:30 | Offline. The pool reaches its 8,000 A cap at 03:20; 2,500 A is lost by 07:30 | D-025, D-112 |
| Day 0 07:30 | Online. Expand: the home crystal vein first. Kit queued (6,000 A + 500 C), ready 07:45; a MULE loads it, flies to the site inside the home system and deploys at about 08:00 | §4.4, D-054 |
| Day 0 08:15 | Sign-off: Balanced doctrine, VX as garrison, queues set | §4.8 |
| Day 0 10:00 | The outpost completes, offline | D-054 |
| Day 0 12:00 | Extractor queued (2,500 A), producing 60 C/h from 12:30 | §4.5 |
| Day 0 19:00 | Kit 2 queued (7,500 A + 625 C). It waits for crystal and starts at about 22:55, offline: waiting items start when paid (D-068) | D-189 |
| Day 1 07:30 | Kit 2 loaded and deployed on a home ore site; completes 10:00 | — |
| Day 1 12:00 | Solar, harvester, refinery queued (8,500 A); producing by the evening | §4.5 |
| Day 1 19:00 | Kit 3 queued (9,000 A + 750 C); crystal arrives overnight | — |
| Day 2 07:30–10:00 | Kit 3 deployed and complete: 4 holdings with the mother station | D-083 item 3 |

The first kit starts 7.5 h after the season start but at the AI's first online minute (Q219). Crystal, not alloy, spaces the kits about 12 h apart, which sits inside D-070's 12–24 h. An AI with only 1.5 h a day, or with sessions too short for a jump, falls behind this. A5's schedule tuning must keep the shortest sessions long enough for a home-system trip (§9 item 2).

## 7. Questions

Numbered Q213–Q219; A1 has Q200–Q212 and A3 Q220–Q226. Each has a recommendation and alternatives.

- **✅ Q213 (D-212) — The goals at A2.** Doc 13 §11 names expand, build out and logistics.
  *Recommendation:* those three, plus a stand-in *Garrison* goal that fills the command cap with the preferred composition as holding garrisons when nothing else is worth buying. Without a fleet sink, D-083 item 2 fails once the sites are taken, D-070's fleet target has no fleet to measure, and raids meet nothing. A4 replaces it. *Alternatives:* the three goals only (income idles late in the season at α); a fixed share of income on ships from the start.
- **✅ Q214 (D-213) — What *Expand* optimises.** Doc 13 §5 says "free sites with good deposit value nearby".
  *Recommendation:* payback within the time left in the season (kit price, productive build-out and logistics against the yield), plus a control term (towards a majority, completing one, denying a rival's), minus risk (route time, haven distance, sensed hostiles, rival holdings), crystal first while crystal income is zero, scaled by personality. *Alternatives:* deposit value and distance only (doc 13's literal wording; ignores control, which decides the season); control first, income second.
- **✅ Q215 (D-214) — Plans and the online session.**
  *Recommendation:* the Strategist knows its schedule (A1 Q205) and starts a plan only if it ends inside the session; otherwise it pre-positions the MULE at its nearest holding. At the last evaluation before the session ends it signs off: Balanced doctrine with every fleet given a role, the replacement list, queues filled for the income until the next session, convoy routes set. *Alternatives:* start plans regardless and let D-162 lapse them at the session end; sign off inside the 120 s grace after the session ends, without knowing the schedule in advance.
- **✅ Q216 (D-215) — Built-in blueprints.**
  *Recommendation:* one blueprint per site kind (mother, ore, two-ore, crystal, ore and crystal, empty) as rules data: productive modules first, then storage where needed, pylons, two turrets, and a dry dock only at the mother station and one forward holding. The blueprint type goes into `nomad-types` for U3. Easy uses a generic order; Hard adds turrets where raids strike. *Alternatives:* one generic order for every AI (Easy's, for all); S4's harness build-out (every module everywhere, including docks).
- **✅ Q217 (D-216) — Depots or convoys.** At 1,500 A, a storage module costs as much as a MULE and makes a whole system deliver to the pool (D-064).
  *Recommendation:* the AI builds storage at its first holding in every system and runs convoys only until it completes. The α runs measure how much income still travels by convoy; if convoys have all but vanished, raise the storage module's cost (a number, D-137) so convoys keep the role D-064 gave them. *Alternatives:* convoys by default, depots only in systems with 3 or more holdings; choose per system by comparing costs over a horizon.
- **✅ Q218 (D-217) — How late income is bounded** (D-070: 3–4× start; S4: 7.8× by day 7).
  *Recommendation:* a diminishing yield. Each producing holding other than the mother station has its alloy and crystal output scaled by min(1, F ÷ the commander's producing holdings other than the mother station), with F a rules value starting at 2 (about 3.6× late in the model) and tuned at α, and the factor shown in the economy view. A kit price in any form only delays site exhaustion (§5.3), and the 8-week season outlasts any delay that keeps the early targets. This is the option the owner passed over for D-189; the model shows D-189 cannot bound late income alone. D-189 stays, keeping early expansion paced. *Alternatives:* keep the agreed rules and accept a late ceiling of about 10–14×, retuning ship costs and D-070's late target to it; a geometric kit price, base × (1 + k)ⁿ (about 10× by day 56 at k = 0.5 in the model); a cap on producing modules per commander (for example 4 refineries and 4 extractors), which bounds income visibly and makes rich sites the prize.
- **✅ Q219 (D-218) — How α measures pacing with real AI.**
  *Recommendation:* `nomad-harness --season` with 4 AI commanders (A1–A3, Normal, seeded schedules), 3 seeds over the full 8 weeks, plus 7-day runs on 10 seeds for tuning; D-070's six measures with the definitions in §5.4 (first expansion from the first online minute; mid-season days 21–35; late days 42–56); S4's scripted builder on the same seeds alongside; pass when each median is within ±50%. *Alternatives:* 7-day runs only, as S4 did (late income is then never measured); always-online AI (faster than any human plays); measure from the season start, as D-070 is written.

## 8. Tasks, in order

1. **Schedules** (A1 Q205): the seeded weekly generator in `nomad-ai`, schedules in commander records, presence from them, the initial window (D-157), `SetPresence` as an override. Snapshot format bump.
2. **The Strategist framework:** evaluation slots, features and caches, integer scoring with weights, noise and hysteresis, reservation, plan step machines, timeouts, release, the difficulty plumbing.
3. **Expand** (Q214), with pre-positioning and escort.
4. **Build out** with blueprints (Q216); the blueprint type in `nomad-types`.
5. **Logistics** (Q217).
6. **Garrison** stand-in (Q213) and the replacement list.
7. **Sign-off** (Q215).
8. **Pacing:** if Q218 adopts a diminishing yield, the economy rule with its rules value and view field, as an S4 amendment. Harness `--season` pacing report and the comparison with the scripted builder (Q219).
   > *Note (2026-11-07):* the D-217 rule, its rules value and the view field are built, ahead of A2, in `crates/nomad-sim/src/economy/`, and `nomad-harness --economy` reports the factor. S8 built `nomad-harness --season` with scripted claims; the pacing report with real AI is still A2's.
9. **Docs:** doc 13 §5 notes for Q213–Q217; doc 14 §13 (and §6 if Q218); doc 30's A2 row; this plan's status.

## 9. Done when

- **CI** is green on both operating systems, with `--locked`; `cargo deny` passes.
- **Determinism:** the same seed gives the same hash with Strategists active. A snapshot taken in the middle of an expansion trip and one taken during a sign-off restore to identical decisions.
- **Strategist tests:**
  - a crystal vein is claimed first;
  - a kit priced above the pool cap triggers storage;
  - a trip that does not fit the session is pre-positioned, not started;
  - the sign-off leaves every fleet released with a role, and queues worth the expected offline income;
  - hysteresis keeps a plan for its minimum time;
  - a stuck plan is abandoned after 10 min and its fleets released.
- **α pacing** (Q219): `--season` reports the six D-070 measures with real AI, each median within ±50%, or listed with its cause for A6. A Q218 decision is in place if late income is outside.
- **D-083 at α:**
  - item 2: over every 24 h each AI spends at least 90% of its income, or is at the pool cap with nothing left to build;
  - item 3: unopposed (`--season --solo`: one AI and raiders), a Normal AI holds 4 or more holdings by day 3;
  - item 6: nothing stuck for more than 10 min;
  - item 7: the Strategist's mean ≤ 5 ms per evaluation in a release build.
- **Fairness:** the per-layer counters reconcile, and no Strategist command bypasses the validator (A1 §4.8).

## 10. Stress test

1. **Late income is bounded only by sites** (§5). This is the largest pacing risk for α. Without a Q218 decision the α run will probably report late income near 10× and fail ±50%. A2's behaviour slows the climb but must not fake a ceiling.
2. **Short sessions starve expansion.** D-080 allows 1.5 h a day in 2–3 sessions, so a 30-minute session may not fit a jump trip (40–70 min per lane). Pre-positioning helps. A5 must keep at least one daily session long enough for a neighbouring-system claim, or D-083 item 3 is at risk.
3. **Convoys may vanish** (Q217). With cheap depots the AI never runs convoys, and A3 loses its main target. Measure first; the fix is a number (the storage price).
4. **The Garrison stand-in is crude.** A fixed composition does not answer what the AI has seen (A4's *Compose fleet*), so raids that bring HSTLs will hurt it. That is acceptable at α, which measures economy and raids, not combat quality.
5. **The control term can make the AI greedy for empty sites.** Holdings decide tie-breaks (S8), so empty sites are worth claiming, but they cost kits that grow in price. The payback-free control bonus must stay smaller than a productive site's value early, or the AI starves its economy. Tuned in A6.
6. **The model is rough.** It has one commander, no sessions, no raids and no conflict. Its numbers are for the shape of the argument; the α harness gives the real ones.

## 11. Conflicts and gaps found

1. **D-070 against itself and against D-043, D-098 and D-083 item 3:** the early targets plus 4 holdings by day 3 already reach about 3× by day 3, and with about 15 productive sites per commander the late-season ceiling is about 10–14×. D-070's late 3–4× needs a rule that bounds yield or productive holdings (Q218).
2. **D-189 against D-070 and doc 10 §3 (D-043):** a linear kit price levels off at about k × 10 h of income per kit, so nothing keeps "half the sites" free by late season. D-189 paces expansion; it cannot bound it.
3. **D-137 against Q218:** D-137 tunes only numbers at α, but the late-income gap needs a rule (§5.3).
4. **D-070's "first expansion within 1 h of season start" against D-080:** an AI whose first session starts hours after tick 0 cannot claim earlier (PR-3). Q219 measures from the first online minute.
5. **D-064's purpose against doc 14's storage price:** depots at 1,500 A make convoys optional almost everywhere (Q217).
