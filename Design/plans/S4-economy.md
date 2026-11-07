# Work package S4 — Economy

Status: **Built** (Q115–Q127 → D-126 to D-138). Code in `crates/nomad-sim/src/economy/`; `nomad-harness --economy` reports the D-070 measures. Two later rule changes are built too: the core-kit price grows with the commander's size (D-189), and remote production follows a diminishing yield (D-217). D-228 moves the peripheral slots wholly outside the dome; that fix comes with S6b. Written together with `S5-combat.md`, because power, modules and module damage are shared between the two. Builds on S3 (entities, rules file, holdings, validator). This plan is also the brief for whoever builds S4.

## 1. Goal

Holdings produce, store, spend and build. Every commander has a pool of alloy and crystal fed by harvesting, refining, extraction and convoys. Each holding has power, build slots and a build queue. Ships and modules are bought and built, the command cap limits the fleet, and the start kit is a real station made of modules. All of it is deterministic, integer and covered by tests, and the harness can measure the D-070 pacing targets, which MVP-α needs within ±50% (D-094).

## 2. Inputs

- **Economy:** doc 14 in full. Agreed: §3 resources (D-065), §4 pool and convoys (D-064), §5 deposits (D-066, D-098), §9.1 placement (D-067), §9.2 queues (D-068), §10 repair (D-071), §11 no control bonus (D-069), §12 start kit (D-112), §13 pacing targets (D-070). Still *Proposed*: §6 harvesting and refining, §7 power, §8 command cap. The §9.3–§9.4 costs, and the numbers in §6–§8, are starting values (D-070). *(Since agreed: §6 as D-126, §7 as D-127 and D-128, §8 as D-129.)*
- **Combat side:** doc 15 §5.2 power routing (D-053), §5.3 abilities, §12 module hull values. Shared interfaces with S5 are in §4.6.
- **Placement:** D-067 and D-119 (slot geometry is designed here), D-100 (dome globe; solar outside), D-046 (pylon azimuths), doc 10 §8 (grid frame, 1 voxel ≈ 35 m).
- **Simulation:** doc 11 §4.1 (economy phase 8 at 1 Hz, staggered), §5 (determinism), D-049 (integer milli-units, `SimId` order), D-104 (rules file copied into the Universe), D-099 item 9 (format changes without migration before β).
- **Offline rules:** doc 12 §3 (peripheral structures are disabled, not destroyed, outside the window), §5.2 (economy doctrine: queues, replacement list, convoy routes).
- **AI:** doc 13 §4 (the Deputy runs convoys, rebuilds and the power profile), §5 (the Strategist's *Expand*, *Build out* and *Logistics* goals read economy state), D-077 (one validator).
- **Code:** `crates/nomad-sim` (`Entities` on `hecs`, `Rules`, `Holding`, the validator in `command.rs`, `TickOutputs`, snapshot format 6) and `crates/nomad-types` (commands; data only, D-086).

## 3. Scope

**In scope:**

- **Modules** as `hecs` entities in build slots, with hull, construction state and a disabled state.
- **Slot geometry** for every holding (Q119, Q120), and the phase-1 module mapping (Q121).
- **The pool:** alloy and crystal per commander in milli-units, with the storage cap (D-025).
- **Production:** harvesters, refineries and extractors at 1 Hz (Q115).
- **Buffers and convoys:** depots, 2 h local buffers, MULE cargo and convoy routes (D-064; Q122, Q123).
- **Power:** supply, demand and allocation per holding under the three routing profiles (Q116, Q117). S5 consumes the allocation.
- **Build queues:** payment at start, waiting items that do not block, cancel refunds, dry-dock lanes (D-068).
- **Ship and module construction,** the rally point, the command cap (Q118).
- **Repair:** free 1%/min, and the REPAIR ability at 8 A/s (D-071).
- **The start kit** as real modules (D-112), replacing S3's `dry_docks` field.
- **Claiming:** core kits, MULE delivery, the 2 h outpost and its completion as a holding (D-054), if Q124 moves it here.
- **Economy events and views** for the AI, the harness and later the UI (§4.8).
- **Harness `--economy`:** builder scripts and D-070 measurements.

**Out of scope:**

- **Weapons, the dome, turret firing, abilities other than REPAIR and PWR ROUTE** (S5). S4 builds turrets and computes their power; S5 makes them fire.
- **Damage to modules** (S5) and the window rule for peripheral structures (S6). S4 reacts to a module being disabled or destroyed.
- **Sieges, capture and outpost combat** (S6).
- **Visibility of other commanders' economies** (S7; ownership is public, contents are not, D-044).
- **AI decisions** (A1, A2): S4 provides the commands and the state they read.
- **Blueprints** (D-035): the data type and an "apply blueprint" command land with A2 or U3, which need them first.

## 4. Design

### 4.1 Units and the economy tick

*Derived* from D-049 and doc 11 §4.1.

- **Amounts** are `u64` milli-units: 1 alloy = 1,000. Rates in the rules file are per hour.
- **Production is exact over time.** Each producer keeps a cumulative-seconds counter for its current rate. Each economy tick credits `⌊rate × (s+1) / 3600⌋ − ⌊rate × s / 3600⌋` milli-units, so there is no drift and no floating point. A rate change, such as a power change or a module being disabled, restarts the counter.
- **Phase 8 runs at 1 Hz per holding,** staggered by a stable hash of the `HoldingId` so the load is spread over 10 ticks (doc 11 §4.1). Power allocation runs in the same slot; S5 reads the last allocation every tick.
- **Overflow:** whatever exceeds the pool cap or a buffer is lost on the tick it arrives (D-025, D-064), and is counted in the economy events, so the harness and the away report can show it.

### 4.2 Modules and slots

*Agreed* placement in slots (D-067); the geometry is *Proposed* (Q119, Q120).

- **A module is an entity:** `Module { holding, slot, kind }`, `Hull { hp, max }` (shared with ships, added in S5a or here, whichever lands first), `Construction { done_at }` while building, and `Disabled { since }` when knocked offline.
- **A slot template per holding kind,** in grid voxels with the origin at the core. Grid north is system north (doc 10 §8).

  | Slot type | Mother station | Other holdings | Where |
  |---|---|---|---|
  | Core | 1 (fixed) | 1 (fixed) | Centre |
  | Pylon | 4 | 4 | On the dome at azimuths 225°/315°/45°/135° (D-046), 26 voxels from the centre, as in phase 1 |
  | Inner, standard | 6 | 4 | Inside the 36-voxel dome; footprint up to 20 × 18 voxels (FA-1's size) |
  | Inner, dock | 1 | 1 | Inside the dome; footprint 34 × 14 (SY-2's size); dry dock only |
  | Solar | 1 | 1 | Outside the dome (D-100: SOL-1 reads as outside) |
  | Deposit | One per deposit (the mother site has 1) | One per deposit (0–2) | On the deposit, outside the dome; its harvester or extractor only |

- **Every core projects the same 36-voxel dome** (Q130), so one template serves all holdings and slot positions never depend on the core type.
- **Inner modules** are refinery, storage, turret, and a second refinery for a two-field site. The dry dock goes only in the dock slot.
- **Build-out of a standard holding** under this template: core kit; 4 pylons; solar; 1 deposit module; refinery, storage and 2 turrets; a dry dock. That costs about 35,000 A and 2,400 C (§5.5).
- **Mapping to the phase-1 scene** (D-119): when the client shows live holdings (C7, U3), modules are drawn at their slot positions, and the phase-1 scene is rearranged to match. Until then the scene stays illustrative. FA-1 is the refinery, SY-2 the dry dock, SOL-1 the solar array, the asteroid field's harvester sits in the deposit slot, and FB-2 is Q121.
- **Hit shapes** for S5 are circles with the footprint's radius, like S3's obstacles (D-105).

### 4.3 Production

*Proposed* in doc 14 §6 (Q115); numbers are starting values (D-070).

- **Harvester:** 600 ore/h × richness, one per ore field (it occupies that field's deposit slot).
- **Refinery:** turns ore into alloy 1:1, up to 900/h, at the same holding. Unrefined ore fills a 1 h ore buffer, then the harvester stalls.
- **Extractor:** 60 crystal/h × richness.
- **Power-limited producers** run at the fraction of their demand that is allocated (§4.5). A disabled module produces nothing and draws nothing.
- **Output** goes to the pool if the holding is in a depot system, otherwise to the holding's buffer (2 h of its production, D-064).

### 4.4 Pool, buffers and convoys

*Agreed* model (D-064); MULE cargo and routes are *Proposed* (Q122, Q123).

- **Pool cap** = mother-station base (4,000 A / 600 C) + each storage module (+4,000 A / +400 C).
- **A depot** is the mother station or a holding with a completed storage module. Holdings in a depot's system deliver straight to the pool.
- **Convoys:** a MULE fleet with a convoy order runs an ordered loop of own holdings that ends at a depot.
  - At each buffer it loads up to its cargo capacity; at the depot it unloads into the pool.
  - A contested gate (D-055) makes it hold at its last stop, or re-route, as doctrine says (doc 13 §4).
  - A convoy is an ordinary fleet: it can be raided (doc 15 §10), and cargo on a destroyed MULE is lost.
- **Core kits** are cargo too: a kit fills a MULE's hold (Q122).

### 4.5 Power

*Proposed* in doc 14 §7 (Q116); allocation is *Proposed* (Q117).

- **Supply** comes from the core (520 MW at the mother station, 120 MW elsewhere) and solar arrays (620 MW each, 0 while disabled). Power never travels between holdings.
- **Demand** comes from pylon base draw (40 MW each), sector regeneration (up to 120 MW per sector at full rate), turrets (40 MW), producers (refinery 120, harvester 60, extractor 60), a building dry dock (150), FLAK SCREEN (90 MW while active) and SHLD SURGE (200 MW for 10 s).
- **Allocation,** recomputed in the holding's economy slot and when an ability starts:
  1. **Fixed draws are all-or-nothing:** pylon base draw, turrets and active abilities. A turret without its full 40 MW holds fire. An ability starts only if its full draw fits after tier 1, and then joins tier 1 until it ends.
  2. **Scalable draws** are served in priority tiers by profile. Within a tier, every consumer gets the same fraction of its demand, computed in integer kW so that rounding is deterministic.

     | Profile | Tier 2 | Tier 3 | Tier 4 |
     |---|---|---|---|
     | Balanced (default) | Regeneration and production together | Building dry dock | — |
     | Shields first | Regeneration | Production | Building dry dock |
     | Production first | Production and building dry dock | Regeneration | — |

  3. **Reported** per holding: supply, demand, allocation per consumer, and load (the phase-1 "1,140 / 1,400 MW · 81% LOAD").
- **S5 reads** regeneration MW per sector and the powered flag per turret (§4.6).

### 4.6 Interfaces shared with S5

*Proposed;* the same text is in `S5-combat.md` §4.6.

| Interface | Owner | Used by |
|---|---|---|
| Module entities with `Hull`, `Disabled`, slot position and hit radius | S4 | S5 hit tests and damage |
| `PowerAllocation` per holding: MW per sector for regeneration, powered flag per turret, ability headroom | S4 (1 Hz, and on ability start) | S5 regeneration, turrets, abilities |
| `request_ability_power(holding, ability)` → granted or refused | S4 | S5 SHLD SURGE and FLAK SCREEN |
| `DamageApplied { target, amount, by }` and the module outcome (disabled or destroyed) | S5 | S4 production and supply; S6 attribution |
| `structure_outcome_at_zero(module)`: disabled or destroyed | S5, with the doc 12 §3 default; S6 adds the window | S5 damage application |
| Pylon count per sector, from completed pylon modules | S4 | S5 sector maximum (Q129) |

### 4.7 Build queues and construction

*Agreed* rules (D-068); details are *Derived*.

- **One queue per holding** for modules and ships, plus one parallel ship lane per completed dry dock. Frigates need a dry dock; small craft build at any core.
- **Payment in full when an item starts.** If the pool is short, the item waits and later affordable items may start. Cancelling refunds 100% before the start and 50% after.
- **Module construction** reserves its slot when queued. The module exists from the start as `Construction` with full hull, draws no power and produces nothing. It completes on a deadline timer (doc 11 phase 2).
  > *Note (2026-11-07), as built:* construction and an outpost's deployment are counted down in the holding's economy second (phase 8), not fired from a phase-2 deadline timer. A dry-dock lane advances by the dock's power share, so its finish tick is not known in advance. That differs from doc 11 §3 and `CODING_STANDARDS.md` §8 (absolute deadline ticks) and is open for the owner.
- **Ships** spawn docked at the holding when done, or at the rally point if docking is full (D-102 capacity). They join a new fleet, or the rally fleet. The command cap is checked when an item **starts** (D-068 timing); a fleet over the cap after losing holdings keeps its ships (doc 14 §8).
- **The validator** gains build, cancel, power-profile, repair, rally, convoy and (with Q124) claim commands, each with rejection reasons and tests, as S3 did.

### 4.8 Events and views

*Derived* from doc 13 §5, doc 17 §6 and doc 20 §5.

- **`TickOutputs.economy_events`:** item started, waiting (short of what), completed, cancelled; storage full and amount lost; buffer full; module disabled, restored or destroyed; holding completed; convoy loaded or unloaded or held.
- **Read-only views** per commander: pool and cap, income per resource over the last hour, command used and cap, and per holding the slots, queue, power report, buffers and deposits. The harness uses them now. S7 wraps them into `CommanderView` (D-077), and the Deputy (A1) and the Strategist (A2) read only that.

### 4.9 Rules file and snapshots

- **The economy section** joins the rules file (D-104): module table, ship costs, production rates, power values, storage, buffers, cargo, cap formula and repair. The rules format version increases.
- **Snapshot format** goes up once per deliberate change. S4 is expected to add one bump (modules, pool, buffers, queues); stability pins are re-recorded in the same commit (D-099 item 9).

## 5. Pacing check against D-070

*Derived* from the starting values; the α harness replaces this estimate with measurements.

| Target | Estimate | Verdict |
|---|---|---|
| Storage fills in 8–12 h | Start: 8,000 A cap at 600 A/h = **13.3 h**. Mid-season (16,000 A, 1,900 A/h) = 8.4 h; crystal 1,800 C at 150 C/h = 12 h | Start is 11% slow; fine at α (±50%) and γ (±25%) |
| First expansion within 1 h | The 6,000 A + 500 C stock is exactly one core kit; the kit is ready at 15 min | Met, with nothing to spare |
| One new holding per 12–24 h early | Second kit at about **14 h**: an extractor on the home crystal vein (2,500 A) is running at 4.7 h, 500 C is there at 13 h, and the alloy at 14.2 h. Third kit about 10 h later | Met, at the fast end. Crystal gates expansion: without the home vein the second kit never comes |
| Full cap fleet in about 24 h of mid-season income | With 5 holdings the cap is **160**, not 200. A 155-command fleet (3 WARDEN, 5 LANCER, 5 VX) costs 28,000 A + 2,300 C: **about 15 h** | 37% fast: inside α, outside γ (Q126). Doc 14 §13's own check assumes a 200 cap that needs 9 holdings |
| Full build-out of a holding in 2–3 days | 35,000 A + 2,400 C (§4.2). At 600 A/h (start, or one standard ore holding's own output): **2.4 days**. At mid-season 1,900 A/h: **0.8 days** | Depends on the reading (Q125) |
| Late income 3–4× start | Late season about 7–8 holdings (half of 60 sites shared by 4, doc 10 §3): mother 600 + about 5 ore holdings × ~510 A/h ≈ **3,150 A/h ≈ 5.3×**, more with hub sites | 30–60% over target (Q126) |

- **Power** at the start: supply 1,140 MW against demand 340 MW (pylons, harvester, refinery), 490 MW with the dock building, and 970 MW with full regeneration. FLAK SCREEN fits (90 MW); a SHLD SURGE (200 MW) needs the regeneration tier squeezed.
- **Remote holdings:** a core's 120 MW cannot run a harvester and a refinery (180 MW), so every productive remote holding also needs a solar array. A standard ore holding then costs 14,500 A + 500 C for +600 A/h, which pays back in about 24 h.

> *Note (2026-11-07):* these estimates assume a flat kit price and full output at every holding. The 7-day harness run showed income at 9.5× start and 16 holdings for one builder, so D-189 made the kit price grow with the commander's size. A kit price alone cannot bound late income, so D-217 also scales each remote producer by min(1, F ÷ the producing holdings other than the mother station), F = 2 to start. Both are built; the harness reports kit prices and the factor.

## 6. Questions

Numbered Q115–Q127; each has a recommendation and alternatives.

- **✅ Q115 (D-126) — Harvesting and refining (doc 14 §6).** As written: refining is local to the holding, one harvester per ore field, a 1 h ore buffer, and power-limited producers slow in proportion.
  *Recommendation:* adopt. *Alternatives:* ore travels in buffers and convoys to refineries at depots; refining only at depots.
- **✅ Q116 (D-127) — Power (doc 14 §7).** As written: supply and demand per holding, no transfer between holdings, the values in the table.
  *Recommendation:* adopt. *Alternatives:* power shared within a system; shared across the commander's holdings.
- **✅ Q117 (D-128) — Power allocation (§4.5).** Fixed draws (pylon base, turrets, active abilities) are all-or-nothing and served first; scalable draws (regeneration, production, the building dock) share by profile tiers, proportionally within a tier. An ability needs its full draw free to start.
  *Recommendation:* adopt. *Alternatives:* one proportional cut for everything; a strict priority per consumer type.
- **✅ Q118 (D-129) — Command cap (doc 14 §8).** 120 from the mother station, +10 per other completed holding, at most 300.
  *Recommendation:* adopt. *Alternatives:* +15 per holding up to 300; a flat 200.
- **✅ Q119 (D-130) — Slot template (§4.2).** Every core projects the same 36-voxel dome. 4 pylon slots; inner slots: 6 standard + 1 dock at the mother station, 4 standard + 1 dock elsewhere; 1 solar slot outside the dome.
  *Recommendation:* adopt. *Alternatives:* 8/6 standard inner slots (more turrets, longer build-out); no dock slot at other holdings (frigates only at the mother station).
- **✅ Q120 (D-131) — Peripheral slots (supersedes D-067's "1–2 peripheral slots").** D-067 puts solar arrays and harvesters in 1–2 peripheral slots, but a site with two deposits plus a solar array needs three. Proposed: one solar slot, plus one deposit slot per deposit for its harvester or extractor.
  *Recommendation:* adopt. *Alternatives:* D-067 as written (a two-deposit site cannot also have solar, so it cannot run a refinery and pylons); a fixed 3 peripheral slots.
- **✅ Q121 (D-132) — FB-2 in the phase-1 station (D-065 against D-112).** D-065 makes FB-2 a second refinery, but D-112's kit has one refinery and one storage module and "matches the phase-1 station". The mother site has one ore field (600 ore/h), so a second refinery would idle.
  *Recommendation:* FB-2 is the storage module (it has a container rack); D-065's FB-2 clause is superseded and D-112 stands. *Alternatives:* FB-2 is a second refinery and the kit gains it; FB-2 is a turret.
- **✅ Q122 (D-133) — MULE cargo.** Doc 14 never sets it, but convoys and core-kit delivery need it.
  *Recommendation:* 2,000 units (1 alloy or 1 crystal = 1 unit); a core kit fills the hold. One MULE then keeps up with one remote 600 A/h holding on a 2–3 h round trip. *Alternatives:* 1,000 units; 4,000 units.
- **✅ Q123 (D-134) — Convoy routes.** A convoy order is a looped list of own holdings ending at a depot. MULEs load buffered alloy and crystal in proportion, up to capacity, and unload into the pool; overflow at the pool is lost as at the source.
  *Recommendation:* adopt. *Alternatives:* crystal first, then alloy; load from the fullest buffer first, whatever the route order.
- **✅ Q124 (D-135) — Claiming moves into S4.** Doc 30 lists outposts under S6, but the expansion target (D-070) and the Strategist's *Expand* goal need core kits, delivery, the 2 h outpost and completion as a holding. S6 keeps the outpost combat exception (D-054) and capture.
  *Recommendation:* move claiming into S4. *Alternatives:* keep all of it in S6 and measure expansion only after S6.
- **✅ Q125 (D-136) — How to read "full build-out in 2–3 days of income" (D-070).** Against the starting income, or one standard holding's own output (600 A/h), the §4.2 build-out takes 2.4 days. Against mid-season income it takes 0.8 days.
  *Recommendation:* the holding's own output (it reads as a payback period, and it is what the costs were sized for). *Alternatives:* mid-season income, which needs about 3× the module costs or more slots; measure both and decide at α.
- **✅ Q126 (D-137) — Pacing deviations already visible (§5).** Late income projects to about 5–6× start (target 3–4×), and a full cap fleet to about 15 h of income (target about 24 h).
  *Recommendation:* keep the starting values for α, measure, and then tune numbers only (ship costs, non-home ore yields). *Alternatives:* adjust now (ship costs ×1.5, poorer ore outside home systems); add a new rule with diminishing returns per extra refinery.
- **✅ Q127 (D-138) — Harvesters under the stakes rule (doc 12 §3).** The doc 12 table lists "harvesters" among things that can be destroyed in real time, but harvester and extractor modules are peripheral structures (doc 14 §9.3), which outside the owner's window can only be disabled.
  *Recommendation:* harvester and extractor modules follow the peripheral rule (disabled outside the window, destroyable inside it); doc 12's "harvesters" means harvesting craft, of which the MVP has none. *Alternative:* harvester modules can be destroyed at any time.

## 7. Tasks, in order

1. **Rules:** the economy section of the rules file and its validation; `Hull` if S5a has not added it. Rules format bump.
2. **Modules and slots:** module entities, the slot template, the start kit as modules (D-112), replacing `Holding::dry_docks`. Snapshot format bump; pins re-recorded.
3. **Pool and production:** pool and cap, exact 1 Hz production, the ore buffer, depots, buffers, overflow accounting.
4. **Power:** supply, demand, allocation by profile, the PWR ROUTE command, and the `PowerAllocation` and ability-headroom interface for S5.
5. **Build queues:** build, cancel, payment and waiting, dock lanes, module construction, ship spawning, the rally point, the command cap.
6. **Convoys:** cargo, convoy routes, contested-gate holding, loss on destruction.
7. **Claiming** (Q124): core kit item, delivery by MULE, outpost construction, completion as a holding with a minimal dome.
8. **Repair:** free 1%/min (no damage in the last 60 s) and the REPAIR ability at 8 A/s.
9. **Reactions to S5:** disabled modules stop producing and supplying; destroyed modules free their slot.
10. **Events and views** (§4.8), and the validator's new rejection reasons, each with a test.
11. **Harness `--economy`:** a scripted builder per commander (expand, build out, convoys) for 7 simulated days, reporting every D-070 measure.

## 8. Done when

- **CI** is green on both operating systems, with `--locked`; `cargo deny` passes.
- **Determinism:** same seed and script give the same hash; snapshot and restore mid-production, mid-build and mid-convoy change nothing; production totals over 24 h equal `rate × 24` exactly (no drift).
- **Economy tests:** the start kit produces 600 A/h; the cap clips and counts overflow; a remote buffer fills in 2 h and stops; a convoy empties it into the pool; power allocation follows each profile, including all-or-nothing turrets; a disabled solar array lowers regeneration and production; queue payment, waiting, cancel refunds and dock lanes follow D-068; the command cap blocks starts, not existing ships.
- **Validator:** every new rejection reason is covered.
- **Pacing:** `nomad-harness --economy` reports the six D-070 measures for 4 builder commanders over 7 days, each within ±50% of its target or listed as a tuning item.
- **Performance:** economy work stays under 5% of the tick budget at stage 1 in that run (doc 11 §4.3).

## 9. Stress test

1. **The numbers already miss two targets** (§5). That is what D-070 expects of starting values, but α must measure them early, since S5's balance depends on how fast fleets regrow.
2. **Crystal gates expansion.** A commander, or an AI, that does not take the home crystal vein first cannot build a second core kit. That is a real decision; the A2 Strategist must know it.
3. **Convoys are the AI's hardest economic job** (doc 14 §14). Q122/Q123 keep routes simple loops; anything smarter is left to the Deputy's rerouting.
4. **The slot template fixes station size.** Free placement stays possible later without changing the economy (doc 14 §14).
5. **Power tiers interact with S5.** If regeneration shares a tier with production, a raid on SOL-1 slows both. That is the intended pressure (D-003), but the TTK targets must be measured with realistic power, not full power.
6. **Claiming in S4** (Q124) moves a package boundary, but it keeps the economy measurable before S6.
