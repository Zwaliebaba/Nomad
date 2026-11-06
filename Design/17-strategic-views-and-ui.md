# 17 — Strategic views and UI

Status: **Proposed (round 10).** Built on the phase-1 package (look, HUD, camera), D-020, D-022, D-032, D-044, D-062, D-064, D-067, D-068, D-072 to D-076 and D-082, and docs 12–16. Decisions needed: Q84–Q87. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Scope

Phase 1 designed the TACTICAL view and its HUD. Its SYSTEM, SECTOR and GALAXY tabs are "visual only" (phase-1 doc 07, item 7).

This document defines:

- those three views;
- how orders are given in them;
- the UI the agreed design needs but phase 1 does not have;
- the final hotkey scheme, which resolves phase-1 doc 07 item 1.

It defines content and behaviour. Pixel layouts follow the phase-1 style and come with implementation.

## 2. Visual language

*Derived* from phase 1. Every view uses the phase-1 language:

- Rajdhani and JetBrains Mono;
- panel style with L-corner accents;
- colours: own cyan, ally green, hostile orange, warning amber;
- shape coding next to colour, as on the phase-1 minimap: own units are squares, hostile units diamonds.

Strategic views use the same **2:1 iso projection** as the phase-1 system panel and minimap, so the game reads as one diorama at every scale.

## 3. View levels and navigation

*Proposed (Q84).*

| Level | Key | Shows | Rotation |
|---|---|---|---|
| **TACTICAL** | F1 | One ops area (phase 1) | 4 snap views (D-062) |
| **SYSTEM** | F2 | One star system: star, orbits (flavour), belt, sites, gates, fleets, sensor coverage | North-up, no rotation |
| **SECTOR** | F3 | The sector's systems and jump lanes | North-up |
| **GALAXY** | F4 | MVP: an overview of the one or two sectors, plus season standings. The full galaxy comes later. | North-up |

- **Drilling down:** double-click a system in SECTOR to open SYSTEM; double-click a site in SYSTEM to open TACTICAL.
- **Moving up:** scrolling out past the lowest zoom step moves up one level.
- The breadcrumb in the phase-1 system panel follows the current level.
- Strategic views stay north-up, like the minimap, so positions can be called out consistently (phase-1 decision).

## 4. SYSTEM and SECTOR contents

*Proposed (Q84).*

**SYSTEM view:**

- **The star** with its glow, orbits as 2:1 ellipses, and the belt (phase-1 system panel style).
- **Sites:** owner colour and shape; holding or outpost; deposit icons showing kind and richness (D-066). A holding shows its exposure window and any Reinforced countdown or siege clock.
- **Gates**, with lane traffic: observed departures and arrival times (D-074).
- **Fleets:**
  - own fleets, live;
  - sensed fleets, live;
  - **last-known** fleets, faded, with their age (D-076).
- **Overlays you can switch on:** sensor coverage, defended zones from doctrine, convoy routes.
- **Control bar:** you / hostile / free (D-041), as in phase 1.

**SECTOR view:**

- **Systems** as iso discs, marked with their kind (home, standard, hub, haven) and coloured by control state.
- **Jump lanes**, labelled with transit time.
- **Fleets:** own fleets in transit with arrival time; observed departures (D-074).
- **Siege and Reinforced markers** with countdowns.
- **Season summary:** systems controlled by each commander, and the early-win threshold.

## 5. Orders in strategic views

*Proposed (Q85).*

- **Selecting:** click or drag a box around fleets in SYSTEM or SECTOR.
- **Giving orders:**
  - right-click a site or gate to move there;
  - right-click a hostile to attack it;
  - right-click a system in SECTOR to **jump** there.
- **Route preview:** a jump shows the route over lanes, the arrival time, and contested gates on the way (D-055).
- **Tactical micro stays in TACTICAL;** strategic views give fleet-level orders.
- All orders are the doc 15 §3.4 order set, sent as ordinary commands (doc 20 §5).

## 6. UI beyond the phase-1 HUD

*Proposed (Q86).* The MVP needs these UI pieces:

| UI | Contents | Source |
|---|---|---|
| **Doctrine editor** | A preset picker (Fortress / Balanced / Trader, D-082), advanced parameters (doc 13 §9), the replacement list with its daily budget, convoy routes, and roles and leashes per fleet. Set per holding, per fleet and for the economy. | Doc 12 §5, doc 13 |
| **Deputy status** (top bar) | "DEPUTY · BALANCED", and whether units are under direct control (RK-10) | D-013 |
| **Away report** (on login, plus a 7-day history) | Raids repelled or lost; losses; disabled structures; Reinforced timers and scheduled sieges in local time; captures; decisions the Deputy deferred; income and how much was lost to full storage. Sorted by importance; each entry jumps to its location. | Doc 12 §5.3 |
| **Window and siege panel** | Your exposure window today and its upcoming shift (D-032); all Reinforced timers involving you; the siege clock during a siege (it replaces the objective timer) | D-015, D-033, D-057 |
| **Build menu and queue** | A slot diagram of the holding (pylon / peripheral / inner slots on the ops grid, D-067); the module catalogue with cost, time and power; reordering and cancelling the queue, with refund rules (D-068); saving and applying blueprints | Doc 14 §9 |
| **Economy panel** | The pool, with its cap and "full in 6 h 20 min"; income per holding; buffers and convoy status (D-064); power per holding with its profile; the command cap | Doc 14 |
| **Fleet panel** | Fleets with composition, current orders, rules of engagement and doctrine role; creating, splitting and merging fleets. It extends the phase-1 roster. | Doc 15 §3.4 |
| **Season panel** | Standings, the season clock, progress toward the early win | D-020, D-041 |
| **Start screen** | Create a Universe (seed, number of AI commanders, difficulty per AI); continue a Universe; catch-up progress; shut down the Universe; "start with Windows" | Doc 20 §2, §7 |
| **Notifications** | The in-client alert banner (phase-1 amber) and toasts for breaches, sieges and raids, plus a **Windows toast while the client is running but minimised**. Notifications that reach you outside the client are hosting work. | Doc 12 §5.3 |

## 7. Hotkeys

*Proposed (Q87); resolves phase-1 doc 07 item 1.*

| Function | Keys |
|---|---|
| **Command card (4×3)** | **Positional grid: Q W E R / A S D F / Z X C V.** Buttons map to keys by position, whatever is selected. With a holding selected, the card shows the phase-1 holding commands. With a fleet selected, it shows fleet orders (Move, Stop, Hold, Patrol, Attack, Jump, Dock, Retreat…). |
| **Camera rotation** | Alt+Q / Alt+E and mouse buttons 4/5. The phase-1 Q/E rotation moves to these keys. |
| **Pan** | Arrow keys, screen-edge scrolling, middle-mouse drag. **WASD panning is dropped**, because W, A, S and D are now card keys. |
| View levels | F1–F4 (phase 1) |
| Control groups | 1–9 to select; Ctrl+1–9 to assign |
| Phase-1 extras | Home, H (HUD on/off), F10 (menu), F11 (fullscreen), F12 (screenshot), unchanged |

*Why:* the phase-1 bindings already conflict (Q rotates the camera and is also SHLD SURGE; W and S are both card buttons and pan keys). A positional grid is the standard RTS answer, and it keeps every card reachable with one hand.

## 8. MVP limits

*Proposed:*

- The GALAXY view stays minimal.
- No chat, because there is no diplomacy (D-026).
- No replay viewer.
- UI scaling is uniform with window size, as in phase 1. Colour-blind safety relies on shape coding.

## 9. Stress test

1. **The amount of UI is large:** nine new panels. The away report and the doctrine editor matter most for the offline thesis (W-01). The economy, fleet and season panels can start as plain lists.
2. **Dropping WASD panning** will annoy players used to it. Arrow keys, edge scrolling and mouse drag remain, and the alternative was a permanent modifier key on every card button.
3. **North-up strategic views next to a rotatable tactical view** can disorient players. The compass in the camera widget and the north-up minimap bridge the two.
4. **The doctrine editor's advanced parameters** can overwhelm. The presets come first, and the advanced settings are folded away by default.

## 10. Effect on other documents

- **Phase 1:** doc 02 key bindings change (Q87). Doc 07 item 1 is resolved.
- **Doc 20:** the start screen and the server lifecycle; notifications arrive as `Notification` messages.
- **Doc 30:** the UI work packages and their order.
