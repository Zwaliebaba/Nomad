# 00 — Glossary

Status: **Draft; round-2 decisions applied.** Terms marked *(phase 1)* come from the owner's phase-1 package. Terms marked *(proposed)* are new and still need confirmation. Unmarked terms are agreed.

## Rule: the total game space is the Universe

The whole persistent game space is the **Universe** (D-008). It is never "the world", "the game world" or "the map". Domain types follow suit: `Universe`, `UniverseState`, `UniverseId`.

*World* is **not** banned as a word. Technical terms keep their usual meaning:

- **World coordinates / world space** (renderer): the frame of the scene being drawn. In the TACTICAL view that is one holding's ops grid, *not* the whole Universe. Do not confuse it with Universe-level coordinates.
- **World labels / world layer** (phase-1 HUD): labels anchored to objects in the scene.
- An ECS `World` type from a library, if we use one.

## Spatial hierarchy

Taken from the phase-1 breadcrumb `ORION ARM › VEIL SECTOR › KESTREL-7 › ORBIT III › KR-114`. Agreed in D-022: **Sector, System and Holding are gameplay layers; Arm and Orbit are flavour and positioning only.**

| Level | Example | Meaning |
|---|---|---|
| **Universe** | — | The one persistent game instance (D-021), for up to 100 commanders. It runs in seasons of 8–12 weeks (D-020). |
| **Galaxy** | — | The playable star map. Each Universe has exactly one galaxy (D-022). |
| **Arm** | Orion Arm | Large region of the galaxy. Flavour only (D-022). |
| **Sector** | Veil Sector | Group of systems; a gameplay layer. The scale of the SECTOR zoom tab (F3). |
| **System** | Kestrel-7 | A star with orbits, a belt and gates; a gameplay layer, and the unit of **system control** that decides a season (D-020). The scale of the SYSTEM zoom tab (F2). The example has 7 holdings. Systems connect only through gates and lanes. |
| **Orbit** | Orbit III | A position band around the star where holdings sit. Positioning only (D-022). |
| **Holding** | KR-114 | A site a commander owns inside a system, built around a station. This is where the TACTICAL view (F1) plays. |
| **Ops grid** | Ops grid 0 | The tactical reference plane of a holding (`z = 0`, voxel units), drawn as an infinite grid. Each holding has exactly one (D-022). |

**Resolved (D-022):** Kestrel-7 is a *system*. The phase-1 fixture's top bar now reads `SYSTEM KESTREL-7`; the reference render still shows the old label (see phase-1 doc 07). "SECTOR 3 / SECTOR 5" in the commanders list are sector names and stay as they are.

## Coordinate frames

| Frame | Units | Use |
|---|---|---|
| **World (grid) coordinates** *(phase 1)* | voxels; `x`, `y` horizontal, `z` up; origin at the command-core centre on the ops grid | Everything inside one holding: modules, ships, effects, rendering |
| **System coordinates** *(proposed)* | to be defined (km or AU) | Positions of holdings, gates and fleets in transit inside a system |
| **Galaxy coordinates** *(proposed)* | to be defined | Positions of systems; lanes between systems |

## Actors

| Term | Meaning |
|---|---|
| **Player** | A person with an account in a Universe. |
| **Commander** *(phase 1)* | The in-Universe identity that owns holdings and fleets. A commander is controlled by a human player, by their Deputy, or by an AI. The HUD shows "4 / 6 COMMANDERS". |
| **Deputy** | The AI layer that **always** executes a commander's doctrine (D-013). Online play adds direct control on top; while the player is offline, the Deputy runs the empire alone. Its authority is fixed in doc 12 §5.1 (D-016). |
| **AI commander** | A commander that no human controls. The MVP has up to 9 (R-05). They play by the same rules with the same information, and simulate online and offline schedules (D-027). Proposal P-02 (still open): an AI commander is built from the Deputy plus a strategic layer. |
| **Ally / hostile** *(phase 1)* | Diplomatic stance as seen from the local commander. Hostiles can be other commanders or NPC raiders (D-014). The MVP has no diplomacy, so every other commander is hostile (D-026, D-027). |
| **NPC raiders** | Non-commander hostile factions that add pressure, especially when few players are online (D-014). The offline rules apply to them too. |

## Assets and structures

| Term | Meaning |
|---|---|
| **Mother station** *(phase 1)* | The commander's core station, for example MS-01 "Aegis". It is never destroyed: if it falls in a siege it makes an **emergency jump** and its site's other modules are destroyed (D-017, D-034). Otherwise stations stay where they are built (D-018). |
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
| **Online / offline** | Whether the human player is connected. It changes only who issues direct orders; protection never depends on it (doc 12 PR-1). |
| **Control state** *(phase 1)* | Who controls a system: CONTESTED, or held by you, a hostile or nobody (free). |
| **Siege state** *(phase 1)* | A holding under sustained attack. |
| **Station-keeping** *(phase 1)* | Holding position relative to a target. Aviation terms are banned (phase-1 rule). |

## Offline play and sieges (doc 12)

| Term | Meaning |
|---|---|
| **Doctrine** | The standing orders a commander writes for holdings, fleets and the economy. The Deputy executes it at all times (D-013). |
| **Exposure window** | The one daily 4 h window in which a commander's holdings' cores can be besieged (D-015). It is derived from the commander's most active 4 h over a rolling 14 days and shifts by at most 1 h per day (D-032). |
| **Breach** | A shield sector reaching 0%. It triggers Reinforced. |
| **Reinforced** | A hardened dome, immune to hostile fire, from the breach until the owner's next exposure window that starts at least 12 h later (D-015). |
| **Siege** | The 60 min decisive phase after Reinforced (D-015). The phase-1 "Hold MS-01 for relief" objective is the siege clock. |
| **Held / Fallen** | Siege outcomes. *Held* means the shield is restored, with 48 h immunity. *Fallen* means the holding is captured, or a mother station emergency-jumps (D-017). |
| **Disabled** | A peripheral structure knocked offline outside the exposure window. It restores itself; it is never destroyed (doc 12 §3). |
| **Vacation** | A limited absence mode: up to 7 days per season, activating 72 h after the request, with no offence and no Reinforced allowed (D-024). Not in the MVP (D-038). |
| **Derelict** | The state of a commander's holdings after 14 days without a login. They have no exposure window, and the Deputy rebuilds nothing (D-023). Not in the MVP (D-038). |
| **Season** | One 8–12 week run of the Universe, won by system control (D-020). |
| **Storage cap** | The limit on stored resources, set so storage fills in about 8–12 h (D-025). |
| **Emergency jump** | How a fallen mother station escapes. Its site's other modules are destroyed; it jumps to the nearest owned holding or a free low-density site; 72 h of protection and a **comeback grant** follow (D-017, D-034). |
| **Blueprint** | A saved station layout. Blueprints carry over between seasons, together with doctrine presets and cosmetics (D-035). |

## Information and diplomacy

| Term | Meaning |
|---|---|
| **Fog of war** | Each commander sees only what their sensors cover (D-039). |
| **Sensor coverage** | The sensor ranges of a commander's holdings, ships and gates. It defines what that commander sees live. |
| **Last-known** | Information outside current sensor coverage, shown with its age in strategic views. |
| **Alliance** | A formal, size-capped group of commanders with shared vision and the right to defend each other's holdings (D-040). Not in the MVP, which has no diplomacy (D-026). |

## Universe structure (doc 10, agreed)

| Term | Meaning |
|---|---|
| **Site** | A fixed location in a system with deposits and a build area. A site owned by a commander is a holding. |
| **Outpost** | A claimed site whose core module is not yet complete. It follows the peripheral rules and does not count for control. |
| **Home / standard / hub system** | System kinds. *Home:* 7 sites, one with a mother station at season start. *Standard:* 5–7 sites. *Hub:* 8–9 sites with richer deposits, at the centre of a sector. |
| **Raider haven** | A system that NPC raiders operate from. Not claimable in the MVP and not counted for control. |
| **Frontier sector** | A reserved sector that opens for players who join mid-season. |
| **Jump lane / trunk lane** | Timed, uninterruptible transit between gates. A jump lane links systems within a sector (40–70 min); a trunk lane links neighbouring sectors (90–150 min). |
| **In-system lane** | A route between gates and sites inside a system, where interception happens. |
| **Controlled / Contested / Open** | System control states. *Controlled:* one commander holds a strict majority of the sites. *Contested:* no majority, and two or more owners. *Open:* no majority, and at most one owner. |
| **Ops area** | The region (proposed radius 20 km) around a site where the tactical ops-grid view applies. |

## Time and simulation (doc 11, agreed)

| Term | Meaning |
|---|---|
| **Tick** | One 100 ms simulation step (10 Hz). Tick 0 is the season start. |
| **Universe time** | UTC, computed from the tick. Used for calendar rules such as exposure windows and season end. |
| **Deadline tick** | How every timer is stored: an absolute tick, never a countdown. |
| **Rate group** | Systems that run below 10 Hz (2 Hz, 1 Hz, every 10 s, every 60 s), staggered across ticks. |
| **SimId** | The stable, never-reused identity of a simulated object. Used for persistence, the protocol and ordering. |
| **Snapshot / journal** | A full saved state every 5 min, plus an append-only log of external inputs flushed at least every second. |
| **Catch-up** | Fast-forwarding the Universe after the server was down, until it reaches the wall clock again. |
| **Freeze** | An operator tool for exceptional outages: shifts all deadlines by the outage instead of catching up. |

## Combat and defence (doc 15, agreed)

| Term | Meaning |
|---|---|
| **Size class** | Light, Medium, Heavy, Structure or Dome. Damage multipliers work against these. |
| **Lock** | Fire-control solution per weapon and target, 0–100%. Spread narrows as lock builds (phase-1 "LOCK 92%"). |
| **TTI** | Time to impact of a projectile. |
| **Cruise mode** | Fast travel outside ops areas while not engaged. Fleets in cruise cannot fire. |
| **Core kit** | What a MULE deploys to claim a site. It becomes an outpost, then a holding when complete. |
| **Jump shroud** | 10 s after arrival through a gate in which a ship cannot be targeted and cannot fire. |
| **Spool-up** | 5 s before departing through a gate, during which a ship can still be hit. |
| **Breach credit** | The commanders who damaged a dome in the 10 min before its breach. They are the siege's attackers. |
| **Time-to-kill (TTK) targets** | The engagement durations that stat tuning aims for. |

## Economy (doc 14, agreed)

| Term | Meaning |
|---|---|
| **Pool** | A commander's single stockpile of ALLOY and CRYSTAL, which all spending draws from. Limited by the storage cap. |
| **Depot** | The mother station, or any holding with a storage module. Holdings in the same system as a depot deliver straight to the pool. |
| **Local buffer** | Up to 2 h of production stored at a holding that has no depot in its system, until a convoy collects it. |
| **Convoy** | A MULE hauler on a logistics route, carrying buffered resources to the pool. Convoys can be raided and blockaded. |
| **Deposit** | An ore field or crystal vein on a site, poor, standard or rich, never running out. |
| **Build slot** | A fixed position on a site's ops grid where one module can be built: pylon, peripheral or inner. |
| **Command cap** | The fleet limit: 120 from the mother station, plus 10 per other holding, up to 300. |

## Information (doc 16, proposed)

| Term | Meaning |
|---|---|
| **Signature** | How detectable a target is by size class (Light 0.6, Medium 1.0, Heavy and Structure 1.5). It multiplies the sensor range. |
| **Shared picture** | Everything any of a commander's sensors detects, available to all of that commander's units. |
| **Visibility set** | The entities a commander currently detects, computed in the 2 Hz sensors phase. |
| **Lane traffic** | Observed departures through a gate the commander covers, with destination and arrival time. |
| **`CommanderView`** | The read-only projection of what one commander knows. It is the only input AI commanders and Deputies get. |
| **Announcement** | Siege information addressed to the owner and the commanders with breach credit. |
