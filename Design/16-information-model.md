# 16 — Information model

Status: **Agreed (round 8).** Q66–Q70 were decided as D-072 to D-076. Built on D-027, D-039, D-041, D-042, D-044, D-052 and D-055, docs 11, 12 and 15, and the phase-1 HUD. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Scope

This document defines:

- what each commander knows, and how they come to know it;
- sensors and detection;
- fleets in transit between systems;
- last-known information;
- what is public;
- how the server limits what it sends;
- how AI commanders and the Deputy are held to the same rules as humans.

Rendering of the strategic views belongs to doc 17; the protocol belongs to doc 20.

## 2. Information classes

| Class | What | Who sees it | Status |
|---|---|---|---|
| **Public** | Galaxy topology: systems, gates, lanes, sites, deposit kinds and richness. Holding ownership. System control states. Season standings. | Everyone | Agreed (D-044, D-041) |
| **Public (proposed additions)** | Each commander's exposure window | Everyone | Agreed (D-075) |
| **Own** | Everything about one's own assets | The owner | Derived |
| **Sensed** | Whatever the commander's sensors detect right now | That commander | Agreed (D-039) |
| **Last-known** | Earlier sensed information, with the time it was seen | That commander | Agreed in principle (D-039); rules agreed (D-076) |
| **Announced** | Reinforced exit times and siege starts | The owner, the commanders with breach credit (D-057), and later the owner's allies | Derived (doc 12 §4, D-057) |
| **Private** | Online status, doctrine, build queues, stockpiles | The owner only (allies decided with D-040) | Agreed (D-075) |

**Why the exposure window should be public:** a breach announces the siege time anyway, so keeping the window secret only adds guesswork. Publishing it lets attackers plan, which is the purpose of D-015. **Why online status should stay private:** showing who is asleep would make raids on sleeping players' peripheral assets more targeted. The stakes rule protects cores, but not convoys.

## 3. Sensors and detection

*Agreed (D-072, D-073).*

- **Detection is binary and 2D:** a target is detected when it is within `sensor range × target signature` of any of the commander's sensors.
- **Signature by size class:** Light 0.6, Medium 1.0, Heavy 1.5, Structure 1.5. A strike group can therefore approach closer, unseen, than a cruiser can.
- **A detected target is fully identified:** class, owner, hull %, fleet membership.
- **No stealth, no ECM and no decoys in the MVP.**
- Detection updates in the 2 Hz sensors phase (doc 11 §4.1). Visibility is computed in the simulation as a per-commander visibility set.

| Sensor | Range (signature 1.0) |
|---|---|
| Holding core | 40 km (phase-1 "TACTICAL / SENSOR · PULSE · 40 KM") |
| VX interceptor | 15 km |
| LANCER strike craft | 10 km |
| WARDEN frigate | 25 km |
| MULE hauler | 8 km |
| Raider strike craft / HSTL cruiser (NPC) | 12 km / 30 km |
| **System control** (D-041) | The whole system, including every gate approach |

- **One shared picture per commander:** anything any of the commander's sensors detects, all of that commander's units can see and target. Weapon ranges (≤ 3.5 km) are far below sensor ranges, so detection rarely limits fire. What it limits is *where* fleets can find each other.
- **Lock** (D-052) builds only while the target is detected.
- The HSTL **ranging laser** (doc 15 §4) needs detection within 4 km.

> **Settled with the S7 plan (D-173 to D-181):** the sensors phase runs for all observers every fifth tick (D-173); detection is per ship, and a fleet shows only its detected members (D-174); a holding is one target that reveals its modules (D-175); sensors are carried by completed cores, ships in space and system control (D-176). Last-known memory keeps one entry per fleet and per holding (D-177, narrowing D-076) and drops a fleet entry when its spot is back in view without it (D-178). Departures (D-179), lost Attack targets (D-180) and NPC memory (D-181) follow the plan.

## 4. Fleets in transit

*Agreed (D-074), building on D-042.*

- A fleet in jump transit is **invisible**. It is a timer, not a position.
- **Departures can be seen:** anyone who currently has sensor coverage of the **origin gate** sees the departure, the destination system and the arrival time. That is what the phase-1 system panel's lane traffic shows ("HSTL WAVE 2 · K-WEST · ~09:30").
- **Arrivals** appear normally at the destination gate approach, for anyone with coverage there. The 10 s jump shroud (D-055) still applies: arrivals are visible but cannot be targeted.
- *Effect:* early warning comes from covering your neighbours' gates, through system control, pickets or scouts. That rewards scouting, and it gives small fleets on picket duty a job.

## 5. Last-known information

*Agreed (D-076).*

- The server keeps, **per commander**, the last observation of every entity that commander has ever detected: position, class, owner, hull and the time seen. This memory is part of commander state, so it is in snapshots (D-048) and survives a reconnect.
- **Expiry:** ships and fleets are dropped from memory after **24 h** without a new sighting. Structures stay until they are seen again, or until public ownership shows the holding has changed hands.
- Strategic views show last-known entries with their **age** ("seen 2 h 14 min ago"). The tactical view shows only live contacts.

## 6. What the server sends

*Derived* from D-039 and doc 11 §9. This is input for doc 20.

- The simulation produces per-commander visibility sets. The server's replication layer sends each client only:
  - public data;
  - the commander's own assets;
  - currently sensed entities;
  - changes to last-known entries;
  - announcements addressed to that commander.

  **An entity the commander has not detected is never sent**, so a modified client gains nothing.
- **Update rates** *(Proposed, detailed in doc 20)*:
  - full rate (10 Hz) for the ops area the player is viewing;
  - 2 Hz for the rest of the systems where the commander has assets or coverage;
  - every 5 s for the galaxy summary.

  Deadlines are sent instead of countdowns (doc 11 §3).

> **Settled with the N4 plan (D-182 to D-187):** the simulation thread extracts each connected commander's view through `CommanderView`, and the network thread diffs and sends it (D-182, amending doc 20 §4). The 10 Hz tier covers the whole system being viewed (D-183). Entries are typed, with velocity so the client can extrapolate (D-184); combat effects are sent as projectile events (D-185); the client reports its focus with `SetFocus` (D-186); deltas carry `since`, and a gap triggers a reconnect (D-187). Protocol version 3.

## 7. AI commanders, the Deputy and raiders see what humans see

*Derived* from D-027 and D-039.

- AI commanders and Deputies get a **`CommanderView`**: a read-only projection of the information classes in §2 for one commander. They never get `UniverseState`. That makes "same information as humans" (D-027) a property of the architecture, not a promise in the code.
- NPC raiders see through their faction's own sensors, under the same rules.
- A test fails the build if AI or Deputy code can reach the full state, for example through crate or module visibility.

## 8. The phase-1 HUD

| HUD element | Source |
|---|---|
| Minimap contacts, "PULSE · 40 KM" | Sensed entities around the viewed holding; the pulse is the core's sensor range |
| Fleet roster "Hostile tracks" | Sensed hostile groups |
| System panel "Lane traffic" | Observed departures and own movements (§4) |
| System panel "System control · 7 holdings" | Public ownership and control (D-041, D-044) |
| "LOCK 92%" | Lock on a detected target (D-052) |
| Siege alert banner | An announcement (§2) |

## 9. Stress test

1. **Binary detection** gives no ambushes beyond what signatures allow. That is a deliberate MVP simplification. Stealth and ECM can be added later as signature modifiers without changing the model.
2. **A public exposure window** removes guesswork, but also some defensive uncertainty. The activity-derived window (D-032) already means the window reflects when the defender really plays. Keeping it secret would only delay the attacker until the breach.
3. **Departure visibility** helps defenders only if they cover their neighbours' gates. That pushes toward controlling or picketing border systems, which is intended. A commander without coverage learns about an attack only when it arrives or when a breach is announced.
4. **24 h fleet memory** is long enough for planning, short enough that stale ghosts disappear.
5. **Per-commander visibility for 100 commanders** costs, per star system, sensors × entities, and is cheap with a spatial grid. It runs at 2 Hz staggered. The pacing simulation measures it.
6. **`CommanderView` adds engineering work:** the AI needs its own queries over a projected view. That cost is what guarantees fairness, and it also keeps AI and Deputy code testable in isolation.

## 10. Effect on other documents

- **Doc 13:** the AI and the Deputy are written against `CommanderView`, including the scouting and picket behaviour from §4.
- **Doc 15:** detection gates lock and the ranging laser. Sensor ranges are added to the unit table.
- **Doc 17:** strategic views show last-known information with its age, plus lane traffic.
- **Doc 20:** interest management, update rates, a per-commander replication filter, and last-known memory in commander state.
- **`nomad-sim`:** a sensors phase that produces visibility sets, and per-commander memory in the state.
