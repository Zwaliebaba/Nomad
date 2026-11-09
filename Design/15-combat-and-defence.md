# 15 — Combat and defence

Status: **Agreed (round 6).** Q46–Q52 were decided as D-052 to D-058. The stat values in §12 are starting values to be tuned (D-058). Built on D-003, D-014, D-015, D-017, D-033, D-034, D-039, D-041, D-042, D-046, D-047 and D-049, docs 10–12, and RK-09. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Scope

This document defines:

- ships and fleets, orders and movement;
- weapons and damage;
- how a holding defends itself (dome, power, turrets, abilities);
- the siege mechanics behind doc 12's lifecycle;
- capture and the emergency jump;
- claims and outposts;
- gate behaviour;
- what NPC raiders may do;
- time-to-kill targets and starting values.

Other documents own the rest: costs, production, power supply and the command cap (14); detection (16); AI behaviour (13); presentation (phase 1 and doc 17).

## 2. Foundations

*Derived:*

- **Space is 2D:** combat happens in the system frame, in metres (doc 10 §8, D-003). Positions and headings are `f64`, with `libm` for transcendental functions (D-049).
- **Ticks:** the simulation runs at 10 Hz, and projectiles travel. Hit tests are *swept*, segment against shape, because a bolt covers about 80 m per tick (doc 11 §10).
- **Targeting needs detection:** you can only target what your sensors currently detect (D-039, doc 16).
- **Offline rules** (doc 12):
  - Ships can be destroyed at any time.
  - Outside the owner's window, structures outside the dome are only *disabled*.
  - Everything inside the dome can be damaged **only during a Siege**.
- **Scale:** 1 voxel ≈ 35 m, so the mother-station dome has a radius of about 1.26 km (doc 10 §8).

*Agreed (D-052):* hull, shield points and damage are integers. Multipliers are integer percentages: `damage = base × multiplier / 100`, rounded down. This follows the same determinism reasoning as integer resources (D-049).

## 3. Ships and fleets

### 3.1 Classes and roles

*Agreed (D-052).* The MVP uses the phase-1 placeholder classes. Roles form a counter triangle: **interceptors beat strike craft, strike craft beat frigates and structures, frigates beat interceptors.**

| Class | Size class | Role | Strong against | Weak against |
|---|---|---|---|---|
| **VX interceptor** | Light | Space superiority, screening | Strike craft | Frigate flak |
| **LANCER strike craft** | Light | Anti-ship and anti-structure torpedoes; breaching domes | Frigates, structures, domes | Interceptors, flak |
| **WARDEN frigate** | Medium | Line ship with flak and a main battery | Interceptors | Strike craft |
| **MULE hauler** | Medium | Logistics, core kits (§8). Unarmed. | — | Everything |
| *Raider strike craft* (NPC) | Light | Raids | Convoys, haulers | Interceptors |
| *HSTL heavy cruiser* (NPC) | Heavy | Raid anchor; ranging laser | Light craft at range | Strike craft |

### 3.2 Damage multipliers

*Agreed (D-052).* Each weapon class has a percentage multiplier against each target class.

| Weapon ↓ / target → | Light | Medium | Heavy | Structure | Dome |
|---|---|---|---|---|---|
| Light cannon (VX) | 100 | 50 | 25 | 25 | 10 |
| Torpedo (LANCER) | 25 | 150 | 150 | 150 | 100 |
| Flak (WARDEN, station) | 200 | 25 | 10 | 10 | 0 |
| Battery (WARDEN, HSTL) | 50 | 100 | 100 | 75 | 75 |
| Turret (station) | 100 | 100 | 100 | 0 | 0 |

### 3.3 Movement

*Agreed with doc 15 (round 6); acceleration and turn rates in D-101; docking in D-102.*

- Each class has a tactical top speed, an acceleration and a turn rate (2D kinematics).
- **Cruise mode:**
  - Outside ops areas, and while not engaged, a fleet travels at **cruise speed**, the speed of its slowest member, and cannot fire.
  - Detecting a hostile within 20 km, or taking fire, drops it to tactical speed. It may cruise again once it has detected no hostile within 20 km and taken no fire for 30 s (D-114).
  - Cruise speeds are chosen so that in-system trips take 2–15 min (doc 10 §4.1) and the 6 h trip limit within a sector holds for the slowest warship (doc 10, invariant 3).
- **Docking:** docked ships sit inside the dome. They cannot be targeted, and they repair over time (the repair rate is set in doc 14). Capacity counts ships: mother-station core 24, other cores 8, +6 per dry dock. Frigates dock only where there is a dry dock (D-102).

### 3.4 Fleet orders

*Agreed with doc 15 (round 6).* The orders are:

- Move
- Attack (target)
- Attack-move
- Patrol (route)
- Guard (object)
- Station-keep
- Dock and Launch
- Retreat
- **Jump** (to a target system; the route over lanes is computed by the simulation)

Rules of engagement are set per fleet: *weapons free*, *return fire* or *hold fire*. A fleet keeps its formation offsets and moves at the speed of its slowest ship. Every ship has a **command cost** (VX 6 and MULE 4, from the phase-1 fixture); the cap comes from doc 14. The same order set is what the Deputy and the AI issue (D-013, D-016).

*Details agreed after S3 (D-113, D-115):* new fleets start on *weapons free*; every docked ship is ready to launch, and a fleet docks all-or-nothing; a mixed fleet's class weapon range is the shortest among its armed ships; order positions lie within the farthest site or gate of the system plus 20 km; a fleet sheds cruise speed inside an ops area at its acceleration; gate spool-up is per fleet, so one ship leaving the 2 km range cancels it for the whole fleet.

## 4. Weapons and damage

*Agreed (D-052).* Details agreed with the S5 plan: a ship's hit radius is half its model's short side (D-143); auto-targeting prefers the highest multiplier, then the nearest, then the lowest `SimId` (D-144); there is no friendly fire (D-146); return fire covers hostiles that hit the fleet or its commander's structures in range in the last 30 s (D-145). SHLD SURGE stays at 20% (D-147); FLAK SCREEN is defined in D-148. S5b's readings (structures need explicit Attack orders, flak never hits structures, a new dome starts full, and more) are D-227. Ties that rules broke by the lower `CommanderId`, such as D-057's capture tie, use a seeded draw instead (D-229).

- **Projectiles:** each weapon has damage per shot, a fire interval, projectile speed, range, and a spread that narrows as **lock** builds. Bolts fly at about 800 m/s, matching the phase-1 HUD's "TTI 1.1 s" at 0.9 km. Torpedoes are slower, about 350 m/s, and do not home.
- **Lock:**
  - Lock is tracked per weapon and target. It rises from 0% to 100% over the weapon's lock time while the target is in range and detected, and resets when the target changes.
  - Spread is interpolated from the base value down to the minimum as lock rises.
  - The spread angle for each shot is drawn from the system's seeded combat stream (D-049). This is the phase-1 "LOCK 92% · TTI 1.1 s".
- **Fire control** aims with linear lead: it predicts where the target will be when the projectile arrives. A target that manoeuvres can therefore dodge.
- **Hit tests:** each tick, a projectile's swept segment is tested against:
  - ship circles, with the radius taken from the model's size;
  - module footprints;
  - the dome circle.
- **Flak:** after its flight time, a flak burst explodes at the aim point with a 150 m radius, damaging every Light and Medium target inside that radius.
- **Ranging laser** (HSTL): the designated target gains lock 50% faster for every allied weapon on it.
- **Applying damage:** all hits in a tick are collected and applied in `(target SimId, projectile SimId)` order. A ship whose hull reaches 0 is destroyed at the end of the combat phase. Wrecks and salvage, if any, are doc 14's call.

## 5. Holding defence

### 5.1 The dome

*Agreed (D-053).*

- The dome is a circle in the system frame: radius = the shield's `radius_xy` (36 voxels for the mother station), centred on the dome centre. It is drawn as a globe centred on the ship plane, and this circle is the globe's equator (D-100).
- It has four sectors, each centred on its pylon (D-046), and each with a strength pool (an integer) and a maximum.
- A hostile projectile that crosses the dome circle damages the sector whose arc it crosses, then stops (phase 1: projectiles stop at the dome).
- **The dome is one-way.** Outgoing fire from turrets and docked defenders launching from inside passes through it.
- **Pylons are inside the dome and cannot be targeted separately.** A sector's strength *is* its pylon's projection.
  > **Superseded by D-255:** pylons stand wholly outside the dome and follow the peripheral rule (doc 12 §3): disabled outside the owner's window, destroyable inside. A lost pylon leaves its sector at the core's 25%; raiders do not target pylons.
- *Agreed with the S5 plan:* a sector's maximum is 25% from the core plus 75% from its own pylon (D-140, superseding D-053's 25% per pylon). Every core has the same dome (D-141). Regeneration is allocated MW × 50/120 per second, scaling with the sector's maximum (D-142); power is allocated as in D-128.
- A sector below 50% shows as weak (phase 1). A sector at 0 is a **breach**, and the holding enters Reinforced (D-015, D-033).
- During Reinforced, the dome is immune to hostile fire (doc 12).
- During a Siege, the breached sector is open: no regeneration, no blocking. Projectiles entering through the open arc can hit modules.
- **Minimal dome** (RK-09): every core module projects a four-sector dome. A core with no pylons has 25% of full sector strength. Each pylon adds 25% of full strength to its sector. The mother station starts with four pylons. *(Superseded in part by D-140: the core gives 25% and each sector's own pylon the other 75%.)*

### 5.2 Power and regeneration

*Agreed (D-053); power values in doc 14 §7.*

- Generators produce power: SOL-1 and others, supply defined in doc 14. Consumers are shield regeneration, turrets, production and abilities.
- **Power routing** (the phase-1 PWR ROUTE button) picks a priority profile: *Balanced*, *Shields first* or *Production first*. When demand exceeds supply, consumers are served in priority order.
- Each sector's regeneration is `min(max regen, allocated power × efficiency)`.
- Disabling SOL-1, a peripheral structure (doc 12 §3), cuts supply and therefore regeneration. That is the phase-1 reason to defend the solar array (D-003).

### 5.3 Turrets and abilities

*Agreed with doc 15 (round 6).* Turrets are station modules with Turret-class weapons. They fire outward through the one-way dome ("TURRETS 4 · 2 FIRING"). Their number is limited by power and build area.

The phase-1 command card maps to these mechanics:

| Button (phase 1) | Mechanic |
|---|---|
| REPAIR (R), "8 A/s" | Repairs modules and docked ships, consuming alloy (rate in doc 14) |
| LAUNCH (L), "6 READY" | Undocks ships that are ready |
| RALLY (Y) | Sets the rally point for new ships |
| ELEV REF (Z) | UI only: the elevation reference plane. Not a combat mechanic. |
| FRIGATE (F) / INTERCEPT (V) / MULE (M) | Production (doc 14) |
| MODULES (B) | Build menu (doc 14) |
| SHLD SURGE (Q), "CD 00:24" | Instantly restores 20% of maximum to a chosen sector. 90 s cooldown, power cost. Has no effect on an open (sieged) sector. |
| FLAK SCREEN (W), "READY · 90 MW" | 10 s of flak bursts around the station. Costs 90 MW while active; 60 s cooldown. |
| PWR ROUTE (P), "SHIELDS PRIO" | Power priority profile (§5.2) |
| STOP (S), "ALL TURRETS" | All turrets hold fire |

The Deputy uses these buttons according to doctrine triggers (doc 12 §5.2), for example SHLD SURGE when a sector drops below 30%.

## 6. Siege mechanics

*Agreed lifecycle (D-015, doc 12 §4); details agreed (D-057).*

- **Breach:** a sector reaches 0, and the holding enters Reinforced. The exit time follows D-015, D-032 and D-033.
- **Attackers of a siege:** every commander whose ships damaged that holding's dome during the 10 min before the breach. At least one of them must be online at siege start, otherwise the result is Held immediately (doc 12).
- **Siege goal:** bring the **core module's hull** to 0 within the 60 min siege clock.
  - Outside a Siege, the core and every module inside the dome are immune to damage.
  - During a Siege they can be damaged. Destroyed modules are lost.
  - Turrets and defenders fight back as normal.
- **Held:** every sector instantly restores to 50% and regenerates normally afterwards; 48 h of immunity follows (D-015).
- **Fallen:** capture, or an emergency jump (§7).

> **Settled with the S6 plan (D-158 to D-168):** the Reinforced exit is the earliest stagger slot at least 12 h after the breach (D-158), and a sector worn to 0 during protection is no breach (D-159). Online at the siege start includes the 120 s grace (D-160). Any commander hostile to the owner can damage modules through the open sector, and capture goes to the most core damage among all of them; NPC raiders deal no damage inside the dome (D-163). A fallen mother station's destination, journey and comeback grant are D-164 to D-167, superseding D-034's destination; what a capture cancels and keeps is D-168.

## 7. Capture and the emergency jump

*Agreed (D-057), detailing D-017 and D-034.*

- **Who captures:** the attacker who dealt the most core damage during the siege. Ties go to the larger total damage dealt to the holding, then to the lower `CommanderId`. This deters a third party from sniping the core in the final second. *(D-229: the last tie-break is a draw from the season's seeded random streams instead of the lower `CommanderId`, so no rule favours the human.)*
- **Normal holding:**
  - The core changes owner and restores to 25% hull; the dome sectors restore to 25%.
  - Surviving modules transfer to the new owner. Build queues are cancelled.
  - The old owner's **docked ships** emergency-undock and jump to the old owner's nearest holding. They are not captured.
  - The new owner gets **48 h of protection**: no Reinforced (RK-09).
- **Mother station** (D-034):
  - The core and the docked ships jump to the destination D-034 defines. The trip takes normal lane transit time, and the station cannot be targeted in transit.
  - The site's other modules are destroyed.
  - The cleared site passes to the attacker as an **outpost**, with a core already deployed and under construction (§8). It does not count for control until complete.

## 8. Claims and outposts

*Agreed (D-054), detailing doc 10 §5 and RK-09.*

- A **MULE** carries a **core kit** (cost in doc 14) to a free site and deploys it. The site becomes an **outpost**, and its construction takes **2 h**. When complete, the site becomes a holding with a minimal dome (§5.1).
- **An outpost under construction can be destroyed at any time.** This is an exception to doc 12's disable-only rule. Without it, nobody could contest a land grab outside the claimant's window. The claimant chose to expose the kit.
- Only one outpost can exist on a site at a time. Outposts do not count for control (D-041).
- *Settled since:* claiming is built in S4 (D-135), and a core kit's price grows with the commander's size (D-189). A core under construction has a hull that grows from 10% to 100% of its maximum over the 2 h (D-224).

## 9. Gates

*Agreed (D-055), detailing D-042.*

- **Arrival:**
  - Arriving ships appear inside the gate approach zone (radius 20 km), scattered up to 2 km from the gate.
  - They have a **10 s jump shroud**: they cannot be targeted and cannot fire, but they can move.
  - This blunts gate camping without making it pointless.
- **Departure:** entering a gate takes a **5 s spool-up**, during which the ship can still be hit, so nobody can escape instantly.
- **Blockade:**
  - Armed hostile ships station-keeping within 5 km of a gate make that gate *contested* for convoys.
  - A convoy holds at its previous stop, according to its doctrine.
  - Fleets can still pass through, using the jump shroud.
- Gates are neutral and cannot be destroyed.

## 10. NPC raiders

*Agreed (D-056); behaviour belongs to doc 13.*

- Raider havens send out raid groups: strike craft, and sometimes a heavy cruiser.
- **Raiders do zero damage to domes, so they can never breach or siege.** This is guaranteed mechanically, not left to AI behaviour.
- Raiders target convoys, ships outside docks, outposts, and peripheral structures. Peripheral structures can only be disabled, as for anyone (doc 12). *(D-223: raiders only disable them, inside the owner's window too, where a commander's hit destroys one, D-169.)*
- Raid strength scales with the target commander's fleet strength (doc 13).
- No new raids start during catch-up (D-047).
- *Settled with the A3 plan:* a raid's launch, target and strength are a simulation rule, and the director AI only steers the spawned group (D-219). Strength comes from the target's fleet value, with a floor of 2 strike craft (D-220); each raid spawns a fresh group (D-221); its 10 minutes start on arrival (D-222).
- *As built (A3):* D-223 is S6b's peripheral outcome, which disables for an NPC attacker at any time; D-224 is the outpost core's hull growing from 10% over its deployment (S4, S6b). An outpost has no dome, so raiders can destroy its core (D-054), but at the floor they cannot in one raid: two strike craft wear 75 a second against a growth of 37.5, and a fresh core starts at 30,000 (A3 plan §12).

## 11. Time-to-kill targets

*Agreed (D-058).* These are the tuning targets. The numbers in §12 are only a starting point; the first pacing-simulation task (D-037) is to tune them until these targets hold.

| Engagement | Target duration |
|---|---|
| Skirmish between small groups (about 6 against 6) | 30–90 s |
| Fleet battle (about 20 ships per side) | 3–8 min |
| Breaching one full-strength mother-station sector with a mid-size fleet (about 10 strike craft and 4 frigates), unopposed | 5–10 min |
| Killing the core during a siege with the same fleet, unopposed | 15–25 min, so that relief arriving in the first half of the 60 min clock still decides the outcome |

## 12. Starting values

*Agreed as starting values (D-058); tuned against §11 in the pacing simulation.*

| Unit | Hull | Tactical / cruise (m/s) | Weapons | Range | Lock | Command |
|---|---|---|---|---|---|---|
| VX interceptor | 600 | 280 / 900 | Light cannon: 8 dmg, 4 shots/s, 800 m/s | 1.5 km | 2 s | 6 |
| LANCER strike craft | 1,400 (D-284; was 800) | 220 / 800 | Torpedo: 160 dmg every 4 s, 350 m/s | 3.0 km | 4 s | 10 |
| WARDEN frigate | 7,500 | 110 / 500 | 2 × Battery: 60 dmg every 2 s; Flak: 30 dmg, radius 150 m, 1/s | 3.0 km / 2.0 km | 3 s | 25 |
| MULE hauler | 2,500 | 120 / 600 | — | — | — | 4 |
| Raider strike craft | 700 | 240 / 850 | Torpedo: 100 dmg every 4 s (0 against domes) | 2.5 km | 4 s | — |
| HSTL heavy cruiser | 30,000 | 70 / 450 | 4 × Battery: 70 dmg every 2 s; ranging laser 4 km | 3.5 km | 3 s | — |
| Turret module | 30,000 | — | Turret: 40 dmg every 1 s | 2.5 km | 2 s | — |
| Core module | 600,000 | — | — | — | — | — |
| Dome sector (4 pylons) | 150,000 | — | Regenerates up to 50/s at full power | — | — | — |

Spread runs from 20 to 4 mrad for cannons and batteries, and from 30 to 6 mrad for torpedoes. Turret range (2.5 km) is deliberately shorter than torpedo range (3.0 km), so a covered strike group can out-range static defences.

> **Tuned since (D-279, D-284):** LANCER hull rose from 800 to 1,400, so strike craft beat frigates at equal value; no ship cost changed. *Derived* from the D-279 study, seeds 1–3 of `nomad-harness --combat triangle`: 12 LANCER against 6 WARDEN now leaves 5 LANCER (before: 3 WARDEN left), 8 against 4 leaves 4 LANCER (before: 2 WARDEN), while 10 VX against 6 LANCER still leaves 9 VX and 6 WARDEN against 20 VX all 6 WARDEN. The triangle is tested at equal value (doc 13 §10, D-083), not count for count: count for count, 6 WARDEN still beat 6 LANCER, and no LANCER hull that wins there keeps VX ahead of LANCER (at 2,400, LANCER beat VX). The 12/8/0 against 8/4/8 battle is a counter test, not a time-to-kill target (D-284). The other §11 times: 6 VX mirror 36 s; 2/2/2 mirror 174 s (was 168 s, still over 90 s); 8/8/4 battle 186 s (was 152 s); station breach and core times unchanged (452 s and 767 s; S9 retunes them, D-277). *Derived:* a raider torpedo does 25 a hit against a LANCER, so a lone raider needs 56 hits to kill one; A3 rechecks raid strength against LANCER escorts.

## 13. Stress test

1. **Defender advantage keeps growing.** A one-way dome, turrets, regeneration, 12 h of notice and relief all favour the defender (RK-02). The levers that work against that:
   - turrets out-ranged by torpedoes;
   - regeneration limited by power;
   - raids on SOL-1 cutting that power;
   - the TTK targets themselves.

   The pacing simulation must show sieges succeeding at a healthy rate.
2. **Hard counters plus fog of war** reward scouting and fleet composition. The multipliers stay moderate. The only zeros are flak against domes, turrets against structures, and raiders against domes, so no matchup is completely hopeless.
3. **Who counts as an attacker** (§6). The 10 min damage window keeps the "an attacker must be online" rule from being dodged by a commander who breaches and then logs off, while still letting a real attacker hand over to a teammate (once alliances exist).
4. **Core sniping in a free-for-all** is addressed by "most core damage captures" (§7).
5. **Jump shroud and spool-up** balance gate camping against instant escape. Both are tuning values.
6. **Performance:** about 200 ships in one battle, at roughly one shot per second each with 3 s flights, means around 600 live projectiles. That is well within budget for one star system.
7. **Balance is unproven.** What we agree on is the TTK targets and the counter triangle, not the numbers.

## 14. Effect on other documents

- **Doc 14:** costs, power supply, cargo and core kits, the command cap, the module list with hull values, repair rates, salvage.
- **Doc 13:** AI tactics using the order set; raider targeting and scaling.
- **Doc 16:** detection gates targeting and lock; the ranging laser's detection range.
- **Phase 1 / doc 17:** the HUD already shows lock %, TTI, the shield-grid sectors and the command-card mapping (§5.3).
- **`nomad-sim`:** once this document is agreed, the first game rules to implement are kinematics, weapons, the dome and the siege lifecycle, then tuning against §11 in the pacing harness.
