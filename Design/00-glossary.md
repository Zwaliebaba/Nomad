# 00 — Glossary

Status: **Draft.** Terms marked *(phase 1)* come from the owner's phase-1 package. Terms marked *(proposed)* are new and still need confirmation.

## Rule: Universe, never World

The persistent game space is the **Universe**. The word *World* is not used: not in documents, not in player-facing text, and not in our own code identifiers. This replaces the common uses as follows.

| Usual term | Our term |
|---|---|
| world (the game) | **Universe** |
| world coordinates, world space (renderer) | **grid coordinates** / **grid space** (the frame local to one holding) |
| world labels, world layer (HUD) | **scene labels** / **scene layer** |
| world state (server) | **Universe state** |
| world map | **galaxy map** (or system map / sector map, depending on the zoom level) |

Third-party Rust crates sometimes expose a type with this name (`bevy_ecs::world::World`, `hecs::World`). If we use one, we hide it behind our own type (for example a `Universe` or `Simulation` facade), so the name never appears in our APIs. See Q16 for the scope of this rule.

## Spatial hierarchy

Taken from the phase-1 breadcrumb `ORION ARM › VEIL SECTOR › KESTREL-7 › ORBIT III › KR-114`. Whether every level is a real gameplay layer is Q4.

| Level | Example | Meaning |
|---|---|---|
| **Universe** | — | One persistent game instance, sized for up to 100 players (R-04). |
| **Galaxy** | — | The playable star map. It is open (Q4) whether one Universe is exactly one galaxy. |
| **Arm** | Orion Arm | Large region of the galaxy. Possibly flavour only. |
| **Sector** | Veil Sector | Group of systems. The scale of the SECTOR zoom tab (F3). |
| **System** | Kestrel-7 | A star with orbits, a belt and gates. The scale of the SYSTEM zoom tab (F2). The example has 7 holdings. |
| **Orbit** | Orbit III | A position band around the star where holdings sit. |
| **Holding** | KR-114 | A site a commander owns inside a system, built around a station. This is where the TACTICAL view (F1) plays. |
| **Ops grid** | Ops grid 0 | The tactical reference plane of a holding (`z = 0`, voxel units), drawn as an infinite grid. The "0" suggests a holding could have more than one ops grid (Q4). |

**Inconsistency to resolve:** the phase-1 top bar says "SECTOR KESTREL-7", but the breadcrumb and the system panel treat Kestrel-7 as a *system*. Separately, the commanders list uses "SECTOR 3 / SECTOR 5". This document assumes Kestrel-7 is a system; Q4 asks for confirmation.

## Coordinate frames

| Frame | Units | Use |
|---|---|---|
| **Grid coordinates** *(phase 1, renamed)* | voxels; `x`, `y` horizontal, `z` up; origin at the command-core centre on the ops grid | Everything inside one holding: modules, ships, effects, rendering |
| **System coordinates** *(proposed)* | to be defined (km or AU) | Positions of holdings, gates and fleets in transit inside a system |
| **Galaxy coordinates** *(proposed)* | to be defined | Positions of systems; lanes between systems |

## Actors

| Term | Meaning |
|---|---|
| **Player** | A person with an account in a Universe. |
| **Commander** *(phase 1)* | The in-Universe identity that owns holdings and fleets. A commander is controlled by a human player, by their Deputy, or by an AI. The HUD shows "4 / 6 COMMANDERS". |
| **Deputy** | The AI that runs a human commander's empire while the player is disconnected. It keeps the standard actions going and defends the commander's assets (R-03). Its exact authority is Q6. |
| **AI commander** *(proposed)* | A commander that no human controls. The MVP has up to 9 (R-05). Proposal P-02: an AI commander is built from the Deputy plus a strategic layer. |
| **Ally / hostile** *(phase 1)* | Diplomatic stance as seen from the local commander. Whether hostiles are players, NPC factions or both is Q5. |
| **NPC faction** *(proposed)* | A non-commander actor such as raiders. It exists only if Q5 says so. |

## Assets and structures

| Term | Meaning |
|---|---|
| **Mother station** *(phase 1)* | The commander's core station, for example MS-01 "Aegis". What happens when it is lost is Q10. |
| **Station** | A structure assembled from modules on an ops grid. |
| **Module** *(phase 1)* | A selectable station component: core spindle, shield pylon, factory, dry dock, solar array, truss. |
| **Core spindle** *(phase 1)* | The centre module of a mother station: habitat ring, docking arms, sensor mast. |
| **Shield pylon / shield sector** *(phase 1)* | Four pylons (N/E/S/W) project a dome. Each projects one sector, and each sector has its own strength. |
| **Power bus** *(phase 1)* | The visible link that carries power from a generator such as SOL-1 to the station. |
| **Elevation tier** *(phase 1)* | Allowed structure heights above the ops grid: 0 / 4 / 8 / 12. Shown as "ELEV +n". |
| **Ship plane** *(phase 1)* | The single height at which all ships fly (`z = 16`). |
| **Fleet / group / wing** *(phase 1)* | A control group of ships, such as CAP WING, INTERCEPT or STRIKE WING. A precise definition is still to come. |
| **Ship classes** *(phase 1, placeholder)* | VX interceptor, LANCER strike craft, MULE hauler, WARDEN frigate; hostile strike craft, HSTL heavy cruiser. |

## Travel

| Term | Meaning |
|---|---|
| **Gate** *(phase 1)* | A system's entry and exit point, for example K-OST or K-WEST. |
| **Lane** *(phase 1)* | A route between gates and holdings that fleets and convoys use. Lanes can be unsafe. |
| **Convoy** *(phase 1)* | An automated transport run, such as the ORE CONVOY. |

## Resources *(phase 1, illustrative)*

ALLOY, CRYSTAL, POWER (MW, used/capacity), COMMAND (fleet capacity), ORE (harvested), MUNITIONS (produced by FB-2). The phase-1 fixture numbers are mock data and are not internally consistent. For example, ALLOY income +312/m equals ORE +312/m, while FA-1 produces only +188/m. The economy design (planned doc 14) will define the real resource chain.

## Status and control

| Term | Meaning |
|---|---|
| **Online / offline** | Whether the human player is connected. When offline, the Deputy is in control. |
| **Control state** *(phase 1)* | Who controls a system: CONTESTED, or held by you, a hostile or nobody (free). |
| **Siege state** *(phase 1)* | A holding under sustained attack. |
| **Station-keeping** *(phase 1)* | Holding position relative to a target. Aviation terms are banned (phase-1 rule). |
