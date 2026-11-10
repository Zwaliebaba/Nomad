# 00 — Glossary

Status: **Draft; decisions up to D-300 applied.** Terms marked *(phase 1)* come from the owner's phase-1 package. Terms marked *(proposed)* are new and still need confirmation. Unmarked terms are agreed; each cites the decision or document that defines it.

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
| **World (grid) coordinates** *(phase 1)* | voxels; `x`, `y` horizontal, `z` up; origin at the command-core centre on the ops grid; 1 voxel ≈ 35 m (doc 10 §8) | Everything inside one holding: modules, ships, effects, rendering |
| **System coordinates** | km in documents and UI, metres in the simulation; 2D, origin at the star, +y = system north (doc 10 §8, D-099) | Positions of sites, gates and ships inside a system; movement and combat |
| **Galaxy coordinates** | abstract 2D layout units (doc 10 §8) | Drawing the galaxy and sector maps only; travel times belong to lanes |

## Actors

| Term | Meaning |
|---|---|
| **Player** | A person with an account in a Universe. |
| **Commander** *(phase 1)* | The in-Universe identity that owns holdings and fleets. A commander is controlled by a human player, by their Deputy, or by an AI. The HUD shows "4 / 6 COMMANDERS". |
| **Deputy** | The AI layer that **always** executes a commander's doctrine (D-013). Online play adds direct control on top; while the player is offline, the Deputy runs the empire alone. Its authority is fixed in doc 12 §5.1 (D-016). |
| **AI commander** | A commander that no human controls. The MVP has up to 9 (R-05). They play by the same rules with the same information, and simulate online and offline schedules (D-027). An AI commander is its Deputy plus a Strategist (D-077, adopting P-02). |
| **Ally / hostile** *(phase 1)* | Diplomatic stance as seen from the local commander. Hostiles can be other commanders or NPC raiders (D-014). The MVP has no diplomacy, so every other commander is hostile (D-026, D-027). |
| **NPC raiders** | Non-commander hostile factions that add pressure, especially when few players are online (D-014). The offline rules apply to them too. |

## Assets and structures

| Term | Meaning |
|---|---|
| **Mother station** *(phase 1)* | The commander's core station, for example MS-01 "Aegis". It is never destroyed: if it falls in a siege it makes an **emergency jump** and its site's other modules are destroyed (D-017, D-034). Otherwise stations stay where they are built (D-018). |
| **Station** | A structure assembled from modules on an ops grid. |
| **Module** *(phase 1)* | A selectable station component: core spindle, shield pylon, factory, dry dock, solar array, truss. In the MVP: core, pylon, refinery, storage, turret, dry dock, solar array, harvester, extractor (doc 14 §9.3). |
| **Core spindle** *(phase 1)* | The centre module of a mother station: habitat ring, docking arms, sensor mast. |
| **Shield pylon / shield sector** *(phase 1)* | Four pylons (N/E/S/W) project a dome. Each projects one sector, and each sector has its own strength. A sector's maximum is 25% from the core plus 75% from its own pylon (D-140). The dome is a globe centred on the ship plane (D-100, D-153). |
| **Peripheral structure** | A module outside the dome: the solar array, harvesters and extractors, each in its own peripheral slot (D-131, D-138), and the four pylons (D-255). Outside the owner's window it can only be disabled; inside it, a commander's hit can destroy it (doc 12 §3, D-169), while NPC raiders only disable (D-223). Its slot lies wholly outside the dome (D-228, D-255). NPC raiders do not target pylons (D-255). |
| **Slot template** | The fixed build slots every holding has: the core, 4 pylon slots, inner slots (6 standard and 1 dock at the mother station, 4 and 1 elsewhere), 1 solar slot and 1 deposit slot per deposit (D-130, D-131). |
| **Docking capacity** | Ships a holding can dock: mother-station core 24, other cores 8, +6 per dry dock. Frigates dock only where there is a dry dock (D-102). |
| **Power bus** *(phase 1)* | The visible link that carries power from a generator such as SOL-1 to the station. |
| **Elevation tier** *(phase 1)* | Allowed structure heights above the ops grid: 0 / 4 / 8 / 12. Shown as "ELEV +n". |
| **Ship plane** *(phase 1)* | The single height at which all ships fly (`z = 16`). |
| **Fleet / group / wing** *(phase 1)* | A control group of ships, such as CAP WING, INTERCEPT or STRIKE WING. In the simulation a fleet is one owner's ships with formation offsets, an order queue and rules of engagement; it moves at its slowest ship's speed and is created, split and merged by command (doc 15 §3.4, S3). |
| **Fleet role** | A fleet's doctrine job: *garrison* (docked at its holding, launches on alert, docks 60 s after it), *reserve* (stays docked), *patrol* or *escort* (run as `Patrol` and `Guard` orders). New ships join the garrison of the holding that built them (D-207). |
| **Rules of engagement** | Per fleet: *weapons free* (the default for new fleets, D-113), *return fire* (only hostiles that hit the fleet, or its commander's structures in range, in the last 30 s, D-145) or *hold fire* (doc 15 §3.4). |
| **Ship classes** *(phase 1, placeholder)* | VX interceptor, LANCER strike craft, MULE hauler, WARDEN frigate; hostile strike craft, HSTL heavy cruiser. |

## Travel

| Term | Meaning |
|---|---|
| **Gate** *(phase 1)* | A system's entry and exit point, for example K-OST or K-WEST. |
| **Lane** *(phase 1)* | A route between gates and holdings that fleets and convoys use. Lanes can be unsafe. |
| **Convoy** *(phase 1)* | An automated transport run, such as the ORE CONVOY. |

## Resources *(phase 1, illustrative)*

ALLOY, CRYSTAL, POWER (MW, used/capacity), COMMAND (fleet capacity), ORE (harvested), MUNITIONS (produced by FB-2). The phase-1 fixture numbers are mock data and are not internally consistent. For example, ALLOY income +312/m equals ORE +312/m, while FA-1 produces only +188/m. Doc 14 defines the real resource chain: ALLOY and CRYSTAL are stocks, ORE is refined 1:1 into alloy, POWER is a per-holding flow and COMMAND is the fleet cap. MUNITIONS is not in the MVP (D-065), and FB-2 is the storage module (D-132).

## Status and control

| Term | Meaning |
|---|---|
| **Online / offline** | Whether the human player is connected. It changes only who issues direct orders; protection never depends on it (doc 12 PR-1). A commander counts as online while connected or inside the disconnect grace; an AI commander by its schedule (D-160). Everyone is offline during catch-up (D-047). |
| **Presence** | Whether a commander counts as online, as the simulation records it (S6a): connected or not, and the grace deadline after a disconnect (D-160). |
| **Control state** *(phase 1)* | Who controls a system: CONTESTED, or held by you, a hostile or nobody (free). The rule is D-041; see *Controlled / Contested / Open* below. |
| **Siege state** *(phase 1)* | A holding under sustained attack. |
| **Station-keeping** *(phase 1)* | Holding position relative to a target. Aviation terms are banned (phase-1 rule). |

## Offline play and sieges (doc 12)

| Term | Meaning |
|---|---|
| **Doctrine** | The standing orders a commander writes for holdings, fleets and the economy. The Deputy executes it at all times (D-013). |
| **Exposure window** | The one daily 4 h window in which a commander's holdings' cores can be besieged (D-015). It is derived from the commander's most active 4 h over a rolling 14 days and shifts by at most 1 h per day (D-032). It is a UTC range that starts on a quarter hour, and it moves only at its own end, towards the best 4 h of the activity history (D-155). It does not move while a holding is Reinforced or under siege (D-156). The human's first window comes from `create --window`, by default local 19:00 (D-157). It follows activity from day 0; in the first 7 days a `SetExposureWindow` choice replaces that day's move (D-157, D-226). Public (D-075). |
| **Activity history** | 14 UTC days of 15-minute buckets that the exposure window follows. A minute counts when the commander is online at the 60 s sample (D-154, D-155); catch-up minutes count as offline (D-171). |
| **Breach** | A shield sector reaching 0%. It triggers Reinforced. While the holding is protected, a sector can be worn to 0 without a breach; the next hostile hit after the protection ends is one (D-159). |
| **Reinforced** | A hardened dome, immune to hostile fire, from the breach until the owner's next exposure window that starts at least 12 h later (D-015). The exit is the holding's stagger slot (D-158). |
| **Stagger slot** | One of the four Reinforced exit times in an exposure window: its start + 0, 1, 2 or 3 h. A breached holding exits at the earliest slot at least 12 h after its breach and after the exits of the commander's earlier-breached holdings, so at most 4 sieges fit in one window (D-033, D-158). |
| **Siege** | The 60 min decisive phase after Reinforced (D-015). The phase-1 "Hold MS-01 for relief" objective is the siege clock. At least one attacker must be online at its start, counting the grace (D-057, D-160). |
| **Held / Fallen** | Siege outcomes. *Held* means the shield is restored, with 48 h immunity. *Fallen* means the holding is captured, or a mother station emergency-jumps (D-017). Held restores every sector to 50% (D-057). On a Fallen holding the hostile commander with the most core damage captures (D-057, D-163). |
| **Protection** | A period in which a holding cannot enter Reinforced: 48 h after Held (D-015), 48 h for a new owner after a capture (D-057), 72 h after an emergency jump (D-034). Each holding's `breachable_from` tick is public (D-170). |
| **Disabled** | A peripheral structure knocked offline outside the exposure window. It restores itself; outside the window it is never destroyed (doc 12 §3). It comes back online at 10% hull once no hostile has been inside its holding's ops area for 10 min, or at once with REPAIR (D-149). It stays targetable, and inside the window a hit destroys it (D-169); NPC raiders only ever disable (D-223). |
| **Disconnect grace** | The 120 s after a disconnect in which a commander still counts as online and direct orders stay valid (doc 12 §5.3, D-160). |
| **Order lapse** | What happens when the grace ends: every direct fleet order lapses, and fleets fall back to doctrine (until A1, they station-keep). Economy queues and convoy routes are doctrine and continue (D-161, D-162). |
| **Vacation** | A limited absence mode: up to 7 days per season, activating 72 h after the request, with no offence and no Reinforced allowed (D-024). Not in the MVP (D-038). |
| **Derelict** | The state of a commander's holdings after 14 days without a login. They have no exposure window, and the Deputy rebuilds nothing (D-023). Not in the MVP (D-038). |
| **Season** | One 8–12 week run, won by system control (D-020). The default is 8 weeks (D-106). Each season is a new Universe (D-116, D-299). |
| **Storage cap** | The limit on stored resources, set so storage fills in about 8–12 h (D-025). |
| **Emergency jump** | How a fallen mother station escapes. Its site's other modules are destroyed; 72 h of protection and a **comeback grant** follow (D-017, D-034). *Destination (D-164, D-165, superseding D-034's "nearest owned holding"):* a free site in the system of the commander's nearest holding by route time; else the free site in the claimable system with the fewest holdings; else it station-keeps by its nearest holding. *Travel (D-166):* an uninterruptible, untargetable transit lasting the route's time. |
| **Comeback grant** | What a commander receives after their mother station falls: the cost of one core kit for each holding they are behind the standings leader, at least 1 and at most 3 kits, paid into the pool past the storage cap (D-034, D-167). |
| **Blueprint** | A saved station layout. Blueprints carry over between seasons, together with doctrine presets and cosmetics (D-035); player blueprints arrive with γ and live in a server table outside the Universe database (D-299). There is also one built-in blueprint per site kind, as rules data (D-215; built with A2). |

## Information and diplomacy

| Term | Meaning |
|---|---|
| **Fog of war** | Each commander sees only what their sensors cover (D-039). |
| **Sensor coverage** | The sensor ranges of a commander's holdings, ships and gates. It defines what that commander sees live. |
| **Last-known** | Information outside current sensor coverage, shown with its age in strategic views. |
| **Last-known memory** | What the server keeps per commander, in the snapshot (D-076): one entry per fleet (composition by class, hull % per class, position, heading, velocity, tick seen) and one per holding (observed modules with kind, slot, hull % and state, tick seen) (D-177). A fleet entry expires after 24 h without a sighting, or as soon as current coverage shows its remembered position empty (D-178). Holding entries stay until re-observed or until ownership changes. NPC factions keep none (D-181). |
| **Alliance** | A formal, size-capped group of commanders with shared vision and the right to defend each other's holdings (D-040). Not in the MVP, which has no diplomacy (D-026). |

## Universe structure (doc 10, agreed)

| Term | Meaning |
|---|---|
| **Site** | A fixed location in a system with deposits and a build area. A site owned by a commander is a holding. |
| **Outpost** | A claimed site whose core module is not yet complete. It follows the peripheral rules and does not count for control. Exception: it can be destroyed at any time (D-054). Its core's hull grows from 10% to 100% over the 2 h deployment (D-224). |
| **Claimable system** | A home, standard or hub system. Raider havens are not claimable and do not count for control or the early win (doc 10). |
| **Home / standard / hub system** | System kinds. *Home:* 7 sites, one with a mother station at season start. *Standard:* 5–7 sites. *Hub:* 8–9 sites with richer deposits, at the centre of a sector. |
| **Raider haven** | A system that NPC raiders operate from. Not claimable in the MVP and not counted for control. |
| **Frontier sector** | A reserved sector that opens for players who join mid-season. |
| **Jump lane / trunk lane** | Timed, uninterruptible transit between gates. A jump lane links systems within a sector (40–70 min); a trunk lane links neighbouring sectors (90–150 min). |
| **In-system lane** | A route between gates and sites inside a system, where interception happens. |
| **Controlled / Contested / Open** | System control states. *Controlled:* one commander holds a strict majority of the sites. *Contested:* no majority, and two or more owners. *Open:* no majority, and at most one owner. |
| **Ops area** | The region (radius 20 km, D-103) around a site where the tactical ops-grid view applies. Cruise mode is not allowed inside it. |

## Season result (S8, agreed)

| Term | Meaning |
|---|---|
| **Standings** | Each commander's systems controlled, holdings owned and deposit value held, ranked in that order (D-041). The last tie-break, "whoever reached the final count first", is the commander whose triple has been unchanged since the earliest tick (D-193). The simulation recomputes them on any tick where completed holdings or their owners change (D-191). Public (D-075). |
| **Early win** | The season ends on the first tick on which a commander controls at least ⌈60% × claimable systems⌉, 6 of 9 at stage 1. That tick becomes the final tick (D-020, D-192). |
| **Near-win warning** | A public announcement when a commander is one system short of the early win, and another when they drop back (D-197). |
| **Final tick** | The season's last tick, set at creation from the season length and carried in `SeasonConfig` (D-195), or the tick of an early win. On it the result is fixed; after it the Universe stops advancing and is served read-only (D-116). |
| **Joint winners** | Commanders still tied after every tie-break; they all win (D-194). |
| **Season result** | The winners, the final tick and the reason (full length or early win), fixed once and written to the `season_result` read model; `standings_history` records every standings change (D-196). |

## Time and simulation (doc 11, agreed)

| Term | Meaning |
|---|---|
| **Tick** | One 100 ms simulation step (10 Hz). Tick 0 is the season start. |
| **Universe time** | UTC, computed from the tick. Used for calendar rules such as exposure windows and season end. |
| **Deadline tick** | How every timer is stored: an absolute tick, never a countdown. |
| **Rate group** | Systems that run below 10 Hz (2 Hz, 1 Hz, every 10 s, every 60 s), staggered across ticks. |
| **SimId** | The stable, never-reused identity of a simulated object. Used for persistence, the protocol and ordering. |
| **Snapshot / journal** | A full saved state every 5 min, plus an append-only log of external inputs and state-hash checkpoints, flushed at least every second. The journal is kept for 48 h (D-110). |
| **State-hash checkpoint** | The state hash of every 600th tick, journaled after that tick's inputs. Replay compares it and logs a mismatch as an error; it is a check, never an input (D-109). |
| **Catch-up** | Fast-forwarding the Universe after the server was down, until it reaches the wall clock again. Also after the PC slept, once the server is more than 1 minute behind UTC (D-117). Journaled `CatchUpStart` and `CatchUpEnd` markers tell the simulation the mode (doc 11 §7). |
| **Rules file** | The unit, movement, economy, combat, siege and season numbers, embedded in the build and copied into the Universe at creation, so a season keeps its rules and replay stays exact. The harness can load another with `--rules` (D-104); AI parameters join it with A1 (D-211). |
| **Freeze** | An operator tool for exceptional outages: shifts all deadlines by the outage instead of catching up. |

## Combat and defence (doc 15, agreed)

| Term | Meaning |
|---|---|
| **Size class** | Light, Medium, Heavy, Structure or Dome. Damage multipliers work against these. |
| **Lock** | Fire-control solution per weapon and target, 0–100%. Spread narrows as lock builds (phase-1 "LOCK 92%"). |
| **TTI** | Time to impact of a projectile. |
| **Cruise mode** | Fast travel outside ops areas while not engaged. Fleets in cruise cannot fire. |
| **Engagement** | The state that keeps a fleet out of cruise mode: it starts when the fleet takes fire or detects a hostile within 20 km, and ends 30 s after the last such trigger (D-114). |
| **Core kit** | What a MULE deploys to claim a site. It becomes an outpost, then a holding when complete. Its price is the base cost × (1 + k × n), where n counts the commander's holdings beyond the mother station, outposts under construction and undeployed kits; k starts at 0.25, and the price is fixed when the kit is queued (D-189). |
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
| **Command cap** | The fleet limit: 120 from the mother station, plus 10 per other completed holding, up to 300 (D-129). |
| **Diminishing yield** | The factor that bounds late income, counted per resource: each holding other than the mother station has its alloy output scaled by min(1, F ÷ the commander's alloy-producing holdings other than the mother station), and its crystal output by min(1, F ÷ its crystal-producing holdings other than the mother station) (D-217, D-281). F is a rules value, now 2.5 (D-282). |
| **Power routing profile** | How a holding shares scalable power: *Balanced* (the default), *Shields first* or *Production first*. Fixed draws (pylon base, turrets, active abilities) are served first, all or nothing (D-128). |
| **Replacement list** | The economy doctrine that rebuilds listed ships and destroyed modules, within a budget: the preset's share of each resource's income over the last 24 h (D-208). |

## Information (doc 16, agreed)

| Term | Meaning |
|---|---|
| **Signature** | How detectable a target is by size class (Light 0.6, Medium 1.0, Heavy and Structure 1.5). It multiplies the sensor range. |
| **Shared picture** | Everything any of a commander's sensors detects, available to all of that commander's units. |
| **Visibility set** | The entities a commander currently detects, computed in the 2 Hz sensors phase. |
| **Lane traffic** | Observed departures through a gate the commander covers, with destination and arrival time. |
| **`CommanderView`** | The read-only projection of what one commander knows. It is the only input AI commanders and Deputies get (D-077). `nomad-ai` reads it through its `View` trait (D-199), and the server extracts each connected client's view through it (D-182). Built with S7. |
| **Announcement** | Siege information addressed to the owner and the commanders with breach credit. As built, announcements also carry window moves and lapsed orders (S6a), and the public control, standings, near-win and season-end changes (S8, D-197). |
| **Focus tier** | The whole system a client is viewing, in TACTICAL or SYSTEM, replicated at 10 Hz. The client names it with `SetFocus`, which changes rates, never visibility (D-183, D-186). |

## AI (doc 13, agreed)

| Term | Meaning |
|---|---|
| **Strategist** | The layer that plays an AI commander while it is "online": utility-scored goals turned into plans of ordinary commands. |
| **Raider director** | The layer that launches and controls NPC raid groups. *D-219:* a raid's launch, target and strength are a simulation rule; the director AI only steers the spawned groups. |
| **Goal / plan** | A scored intention (expand, defend, siege…) and the sequence of ordinary commands that carries it out. |
| **Personality** | Seeded parameters (aggression, expansion appetite, risk tolerance, preferred composition) that make AI commanders play differently. |
| **Doctrine preset** | Fortress, Balanced or Trader: a ready-made set of Deputy parameters. |
| **`nomad-ai` / `View`** | The crate that holds all AI logic. It depends only on `nomad-types` and reads the Universe through a `View` trait that `nomad-sim` implements over `CommanderView`, so AI code cannot name `UniverseState` (D-199). |
| **Outbox / inbox** | Per-owner queues in the state. AI layers write commands to the outbox, applied in phase 1 of a later tick after external inputs (D-200). The inbox holds the events the owner may know about, for 60 s (D-202). |
| **Competence cap** | The limit on how fast AI layers act: a latency of 10, 6 or 3 ticks by difficulty (3 for a human's Deputy), and at most 1, 2 or 4 commands in any 10 consecutive ticks (4 for a human's Deputy), shared by all of a commander's AI layers (D-201). |
| **Raid group** | The NPC ships one raid spawns at a haven's gate and removes when they return (D-221). Its strength is 30–50% of the target's **fleet value** in that system (D-081): build cost as alloy + 3 × crystal, docked ships included, with a floor of 2 strike craft (D-220). |

## Server and network (doc 20, agreed)

| Term | Meaning |
|---|---|
| **Connection file** | `server.json` in a Universe's directory, written by the running server: the address a local client connects to, the certificate fingerprint and the token, with the Universe, the build, the protocol version and the process ID. It is removed on a clean stop (D-120, D-150). The certificate fingerprint goes when the QUIC endpoint retires with N6 (D-242); the token stays for sessions without an `Origin` (D-244). |
| **Import string** | The same details as a `nomad://` string, printed by `nomad-server connect-info`, for a client that cannot read the server's files, such as one outside a container. It stays valid until `rotate-token` (D-151, D-152). Retired with the QUIC endpoint (D-242): a browser reaches the server at its loopback address and needs no import (D-244). |
| **Identity** | The server's self-signed certificate, its key and the session token, made once per Universe and kept in `identity.json` (D-151), private to the user (D-122). The certificate and its key retire with the QUIC endpoint (D-242); the token stays (D-244). |
