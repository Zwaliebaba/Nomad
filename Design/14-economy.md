# 14 — Economy

Status: **Agreed (round 7).** Q58–Q65 were decided as D-064 to D-071. Numbers are starting values, tuned against the pacing targets (D-070). Built on D-016, D-019, D-020, D-025, D-035, D-041, D-043, D-049 and D-053, docs 10, 12 and 15, and W-08. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Scope

This document covers the items earlier documents deferred to the economy:

- resources, and where they are stored;
- deposits on sites, which also feed doc 10 invariant 5;
- harvesting and refining;
- power supply and demand;
- the command (fleet) cap;
- modules and how they are placed;
- build queues;
- costs, build times and power draw;
- repair, salvage and upkeep;
- the season start kit;
- economy pacing targets.

Combat mechanics stay in doc 15; AI economic behaviour belongs to doc 13.

## 2. Constraints already agreed

*Derived:*

- **Time:** the economy runs in real time. Builds take minutes and the economy ticks at 1 Hz (D-019, doc 11 §4.1). Amounts are integer milli-units (D-049).
- **Storage caps:** storage fills in 8–12 h of normal income, and income beyond the cap is lost unless spent (D-025). That bounds what accumulates offline (W-08).
- **The Deputy** continues build queues and convoy routes, and rebuilds losses from an owner-approved replacement list within a daily budget (D-016). It never starts new projects.
- **Seasons** reset assets; blueprints and doctrine presets carry over (D-020, D-035).
- **About 15 sites per commander** (D-043). Holdings need a completed core to count for control (D-041).
- **The phase-1 fixture numbers are mock data** (glossary). Phase 2 feeds the HUD from the simulation, so the numbers in this document replace them.

## 3. Resources

*Agreed (D-065).*

| Resource | Kind | Source | Used for |
|---|---|---|---|
| **ALLOY** | Stock | Refined from ore | Ships, modules, core kits, repair |
| **CRYSTAL** | Stock | Extracted from crystal veins | Frigates, pylons, turrets, core kits |
| **ORE** | Intermediate flow, not shown in the top bar | Harvested from asteroid fields | Refined 1:1 into alloy |
| **POWER** | Flow (MW); generation against demand; not stored | Core, solar arrays | Shield regeneration, turrets, factories, abilities (doc 15 §5.2) |
| **COMMAND** | Cap | Mother station and holdings | Total command cost of a commander's ships |

**MUNITIONS** (phase-1 FB-2) is **left out of the MVP**. In the MVP, FB-2 becomes a second refinery. Ammunition logistics would add a supply chain to every weapon. It can come back later as a separate design.

## 4. Where resources are stored

*Agreed (D-064): a pooled stockpile, fed by convoys from remote holdings.*

- **Spending comes from one pool per commander,** as the phase-1 top bar shows. A build at any holding draws from the pool when it starts.
- A **depot** is the mother station, or any holding with a storage module. Holdings **in the same system as a depot** deliver straight into the pool.
- Every other holding fills a **local buffer**. The buffer holds 2 h of that holding's production, and anything beyond that is lost.
- **Convoys** are MULE haulers on a logistics route (the phase-1 "LOGISTICS · 2 × MULE · HAULING"). They empty buffers into the pool. The Deputy runs the routes from doctrine (D-016).
- Convoys can be **raided** (doc 15 §10) and **held by blockades** (D-055). A commander without MULEs on logistics leaves remote income in buffers that fill and stop.
- **Why this model:**
  - Spending stays simple: one pool and one top bar.
  - Raids, blockades and the phase-1 convoy all get real stakes.
  - Expansion into another system becomes a choice: build a depot there, or run convoys.
- *Rejected alternatives:*
  - **Pooled with no convoys:** raids and blockades lose most of their meaning.
  - **Fully local stockpiles:** spending becomes logistics micro, too heavy for the MVP and the AI.

**Storage cap** (D-025): the pool's cap is the mother station's base plus each storage module. It is sized so that normal income fills it in 8–12 h (§10).

## 5. Deposits

*Agreed (D-066).*

- **Each site has 0–2 deposits.** A deposit is either an ore field (asteroids, like AST-1) or a crystal vein. Its richness is *poor* (×0.5), *standard* (×1) or *rich* (×1.5).
- **Deposits never run out** in the MVP. They are limited by rate, not by quantity. Depletion, which might one day push commanders to migrate, is a possible later design.
- **Distribution** by system kind (doc 10 §4.1):
  - **Home:** the mother-station site has 1 standard ore field. The other home sites have mostly poor and standard ore, and at most 1 crystal vein.
  - **Standard:** standard ore, plus 1–2 crystal veins.
  - **Hub:** rich ore and rich crystal. That is what makes it worth contesting.
- **Counts** (D-098): ore fields per site are drawn as 0/1/2 with weights 1:2:1; every home system has exactly one crystal vein; standard-system veins are Standard richness; the hub has 2–3 crystal veins and ore on every site; the mother-station site holds only its Standard ore field.
- **Deposit value**, used for doc 10 invariant 5 and the season tie-break in D-041: ore field = 1 × richness, crystal vein = 3 × richness. The generator keeps every home neighbourhood's total value within ±10% of the others.

## 6. Harvesting and refining

*Proposed.*

- A **harvester module** on a site with an ore field extracts ore at 600 ore/h × richness. There is one harvester per ore field.
- A **refinery module** converts ore into alloy 1:1, up to 900 per hour. Ore that is not refined collects in a small ore buffer (1 h), then stops.
- An **extractor module** on a crystal vein produces 60 crystal/h × richness.
- Production runs at 1 Hz with integer milli-unit remainders (D-049). When power is short, factories slow down in proportion to the power allocated to them under the active routing profile (doc 15 §5.2).

## 7. Power

*Proposed.*

| Source | Output |
|---|---|
| Mother-station core | 520 MW (phase-1 "CORE 520") |
| Other cores | 120 MW |
| Solar array (peripheral, outside the dome) | 620 MW (phase-1 "SOL 620") |

| Consumer | Draw |
|---|---|
| Shield pylon | 40 MW each, plus up to 120 MW per sector for regeneration at full rate |
| Turret | 40 MW |
| Refinery / harvester / extractor | 120 / 60 / 60 MW |
| Dry dock while building | 150 MW |
| FLAK SCREEN while active | 90 MW (phase-1) |
| SHLD SURGE | 200 MW for 10 s |

Supply and demand are per holding, so power does not travel between holdings. The top bar shows demand against supply for the selected holding (phase-1 "1,140 / 1,400 MW · 81% LOAD").

## 8. Command cap

*Proposed.* The cap is **120** from the mother station, plus **10 per other completed holding**, up to **300**.

The fleet grows with expansion, but only sublinearly: holdings beyond the 18th add nothing. Command costs come from doc 15 §12 (VX 6, LANCER 10, WARDEN 25, MULE 4). Losing holdings lowers the cap. Nothing is destroyed, but no new ships can be built while the fleet is over the cap.

## 9. Modules, placement and building

### 9.1 Placement

*Agreed (D-067).*

- Modules go into **build slots**, which are fixed positions on each site's ops grid at the phase-1 elevation tiers (0 / 4 / 8 / 12). The number of slots depends on the site (doc 10 §5, "build area").
- **Slot types:** 4 pylon slots around the dome edge; 1–2 peripheral slots outside the dome, for solar arrays and harvesters; inner slots for everything else.
- Trusses are generated automatically between modules and are cosmetic.
- A **blueprint** (D-035) is a saved assignment of modules to slots plus a build order.
- *Why slots rather than free placement:* free placement needs collision and adjacency rules, tier rules for every module type (phase-1 doc 07, item 6), placement UI and an AI that lays out stations. Slots keep the phase-1 diorama look at a fraction of that cost.

### 9.2 Build queues

*Agreed (D-068).*

- Each holding has **one build queue** for modules and ships, as the phase-1 MS-01 queue shows. Each **dry dock** adds one parallel lane for ships.
- **Payment:** an item is paid in full when it starts building. If the pool is short, the item waits; it does not block items after it that can be afforded.
- **Cancelling:** 100% refund before the item starts, 50% after.
- Small craft (VX, LANCER, MULE) can be built at any core. Frigates need a dry dock.

### 9.3 Module list (starting values)

| Module | Slot | Cost | Build time | Power | Hull | Effect |
|---|---|---|---|---|---|---|
| Core, mother station | — | — (start kit) | — | +520 MW | 600,000 | Dome, base storage 4,000 A / 600 C, 120 command, docking for 24 ships (D-102) |
| Core, other holdings | — | Core kit: 6,000 A + 500 C | 15 min to build the kit, plus 2 h deployment (D-054) | +120 MW | 300,000 | Minimal dome (D-053), base storage for a buffer, +10 command, docking for 8 ships (D-102) |
| Shield pylon | Pylon | 2,500 A + 300 C | 30 min | 40 MW | 40,000 | +25% strength in its sector (D-053) |
| Solar array | Peripheral | 3,000 A | 30 min | +620 MW | 20,000 | Disabled, never destroyed, outside the window (doc 12) |
| Harvester | Peripheral | 2,000 A | 20 min | 60 MW | 15,000 | 600 ore/h × richness |
| Refinery | Inner | 3,500 A | 40 min | 120 MW | 40,000 | Up to 900 alloy/h |
| Crystal extractor | Peripheral | 2,500 A | 30 min | 60 MW | 15,000 | 60 C/h × richness |
| Storage (container rack) | Inner | 1,500 A | 15 min | 0 | 20,000 | +4,000 A / +400 C to the pool cap; makes the holding a depot |
| Turret | Inner | 2,000 A + 200 C | 20 min | 40 MW | 30,000 | Doc 15 §12 turret |
| Dry dock | Inner | 5,000 A + 300 C | 60 min | 150 MW while building | 50,000 | One parallel ship lane; frigates; +6 docking (D-102) |

### 9.4 Ship costs (starting values)

| Ship | Cost | Build time | Command |
|---|---|---|---|
| VX interceptor | 1,200 A | 3 min | 6 |
| LANCER strike craft | 2,000 A + 100 C | 5 min | 10 |
| WARDEN frigate | 4,000 A + 600 C | 20 min | 25 |
| MULE hauler | 1,500 A | 5 min | 4 |

The relative costs keep the phase-1 command card's proportions (FRIGATE 400 A · 60 C, INTERCEPT 120 A, MULE 150 A), scaled ×10 so they fit a 24 h income cycle.

## 10. Repair, salvage and upkeep

*Agreed (D-071).*

- **Repair:**
  - Docked ships and undamaged-in-the-last-60-s modules repair **free at 1% of hull per minute**.
  - The REPAIR ability (doc 15 §5.3) speeds this up at **8 A/s** (phase-1 "8 A/s"), restoring 80 hull per second spread across the most-damaged modules and docked ships.
- **Salvage: none in the MVP.** It would add pickup micro-management, and it rewards snowballing.
- **Upkeep: none.** *Derived* from D-025: storage caps were chosen instead of upkeep.

## 11. System control and the economy

*Agreed (D-069): no economic bonus for control.*

Control already decides the season (D-020) and gives sensor coverage (D-041). Adding an income bonus would make the leader's lead grow faster.

## 12. Season start kit

*Agreed (D-112).*

- **Mother station:** core, 4 pylons, 1 solar array, 1 harvester on its ore field, 1 refinery, 1 storage module, 1 dry dock. This matches the phase-1 station.
- **Fleet:** 3 VX and 2 MULE.
- **Stock:** 6,000 A and 500 C, enough for one core kit, so the first expansion can start within the first hour.
- **Starting income:** about 600 A/h and about 0 C/h, so crystal must come from expansion.

## 13. Economy pacing targets

*Agreed (D-070).* As with doc 15's time-to-kill targets, these targets are what gets agreed. The numbers in §6–§12 are tuned until they hold in the pacing simulation (D-037).

| Target | Value |
|---|---|
| Storage fills at normal income | 8–12 h (D-025) |
| First expansion started | Within 1 h of season start |
| New holdings early in the season | About one per 12–24 h |
| Rebuilding a full command-cap fleet from nothing | About 24 h of mid-season income |
| Full build-out of a standard holding | 2–3 days of income |
| Late-season income versus starting income | 3–4× (a bounded snowball) |

*Rough check:*

- Mid-season, with about 5 holdings and 3 refineries, income is about 1,500–2,000 A/h and about 150 C/h.
- A cap fleet of about 200 command (4 WARDEN, 6 LANCER, 6 VX) costs about 35,000 A and 3,000 C. That is roughly 20–24 h of income.
- **Storage:** the start cap of 8,000 A (core plus one rack) fills in about 13 h at the starting income of 600 A/h. With two more racks (16,000 A), it fills in about 8.5 h at mid-season income of 1,900 A/h. Both stay close to the 8–12 h target.
- The start cap of 8,000 A must hold the largest single cost, a 6,000 A core kit. It does.

## 14. Stress test

1. **Convoys add AI and Deputy work** (logistics routes, re-routing around blockades). Without convoys, raids and blockades would be close to meaningless. Mitigation: depots let a commander avoid convoys entirely, at the cost of a storage module per system.
2. **Rate-limited, never-ending deposits** remove any reason to move on. That is fine for 8–12 week seasons; depletion stays a possible later design.
3. **The command cap limits snowballing.** The pool cap limits hoarding. Without upkeep, a commander at the cap with full storage has nothing left to spend on except modules. Build-out (2–3 days per holding) and expansion are the intended sinks. *Watch in pacing:* if commanders hit the cap with idle income by mid-season, add sinks (module upgrades) rather than upkeep.
4. **Slots constrain creativity** in station design. That is accepted for the MVP. Free placement can come later without changing the economy.
5. **Paying at build start** means an attacker sees no stockpile to steal, and the defender loses only what was in queues (cancelled on capture, D-057). That keeps captures from turning into economic wipe-outs.
6. **Power per holding** makes SOL-1 raids matter locally (D-003) without cascading across the empire.

## 15. Effect on other documents

- **Doc 10:** invariant 5 can now be implemented with the deposit values in §5. Site build areas become slot sets.
- **Doc 12:** convoy routes and the replacement list connect to §4 and §9. The Deputy's daily budget is a share of income, set by doctrine (doc 13).
- **Doc 13:** the AI economy covers expansion, slot build orders, convoy routing and the replacement list.
- **Doc 15:** power values (§7) and module hull values (§9.3) are filled in.
- **Phase 1:** the fixture's resource and cost numbers are mock data and will be replaced when the HUD is fed by the simulation.
