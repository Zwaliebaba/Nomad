# 04 — Design review (round 3)

Status: **Decided (2026-11-06).** Every concern has a decision (D-032 to D-040) or a home in the named design document. Residual risks are noted per item. This is a critical review of the design as agreed so far: D-001 to D-031, doc 12 and the phase-1 package. Concerns are ranked by impact. Each one carries a recommendation. Where the owner needs to decide something, the question is in `02-open-questions.md` (Q28–Q34). Concerns that only need a rule written down go to the design document named.

| ID | Concern | Impact | Recommendation | Outcome |
|---|---|---|---|---|
| RK-01 | Players can choose an exposure window when their enemies are asleep ("time-zone tanking") | High | Derive the window from the commander's actual play hours | Q28 → D-032 (activity-derived window) |
| RK-02 | Conquest pacing is unmodelled; one full attack loop takes at least 13 h at 1× | High | A strategic time scale (combat stays 1:1) plus a headless pacing simulation | Q29 → D-036 (no compression; rejected), Q30 → D-037 (pacing simulation). *Residual:* human playtests of a siege cycle take at least 13 h |
| RK-03 | The MVP is not "limited": 9 competent AI commanders and strategic map views are each major projects | High | Stage the MVP, starting with fewer AI commanders; cut features that do nothing with one human | Q31 → D-038 (staged MVP) |
| RK-04 | Building phase 1 first tests the part we are most confident in and defers the real risks | Medium–high | A headless simulation prototype in parallel with phase 1 (refines D-028) | Q30 → D-037 (parallel sim track) |
| RK-05 | There is no information model (fog of war, sensors), yet PvP, AI fairness (D-027) and networking all depend on one | High | Sensor-based fog of war per commander | Q32 → D-039 (sensor-based fog of war) |
| RK-06 | "Never eliminated" can turn into "permanently crippled"; D-017 is ambiguous about the lost modules | Medium | Modules are destroyed, not captured; 72 h of protection after a jump; a defined jump destination | Q33 → D-034 |
| RK-07 | Season resets erase the station a builder spent weeks on | Medium | Carry over blueprints, doctrine and cosmetics, not assets | Q34 → D-035 |
| RK-08 | Travel time and the 12 h notice depend on each other; relief only works if fleets can arrive in time | Medium | Rule: any trip within a sector takes less than half the notice | Doc 10 |
| RK-09 | The stakes rule assumes every holding has a dome; new claims are undefined | Medium | Every holding has a core module with a minimal dome; anything without one is an outpost and follows the peripheral rules | Doc 15 |
| RK-10 | With an always-on Deputy, direct orders and doctrine can conflict | Medium | A direct order wins until the unit is released; doctrine safety triggers are opt-in per order | Doc 12 addendum |
| RK-11 | A single Universe with seasons gives players arriving mid-season no entry point | Low–medium | A protected frontier sector for late joiners | Doc 10 |

## RK-01: Time-zone tanking

D-015 lets each commander pick their own daily 4 h window. A rational player in a contested region picks 04:00–08:00, when their enemies are asleep. They then become nearly impossible to take, and their Deputy rarely has to fight a siege at all. Some MMOs with player-chosen vulnerability windows know this exploit well. It undermines the purpose of the window, which is to schedule a fight *both* sides can attend.

**Recommendation:** the window is the commander's most active 4 h, measured over a rolling 14 days, and it moves at most 1 h per day. New commanders choose freely until they have 7 days of history. This keeps PR-1 intact: protection still depends on the clock, never on live connection status. Gaming the window then requires actually playing at those hours, which is exactly the outcome we want. *Alternative:* restrict windows to a "prime time" band in Universe time, which is simpler but unfair to some time zones.

## RK-02: Unmodelled pacing

Rough bounds under D-015:

- **Fastest capture** of one holding: the breach, at least 12 h of notice, then a 60 min siege. That is about 13 h.
- **After a failed siege:** 48 h of immunity, then a new breach and new notice. So each failed attempt costs the attacker at least about 2.5 days on that holding.
- **Fastest swing:** with staggered exits (Q26), one defender can lose up to 4 holdings in a single window.

Both stagnation (turtles never fall) and runaway (one bad evening flips a system) are plausible. Nothing tells us which happens before code exists. On top of that:

- One complete loop (breach → Reinforced → siege → capture) takes at least 13 real hours at 1×, so iterating on gameplay is very slow.
- A season (8–12 weeks) is far too long for MVP playtests.

**Recommendation:**

1. *(Rejected by the owner: D-036.)* Add a **strategic time scale** per Universe. It scales the strategic timers: notice, immunity, travel, builds, storage fill and season length. The simulation tick and combat stay at 1:1, so D-019 still holds for everything a player sees in a fight. The exposure window itself stays tied to the real clock.
2. *(Accepted: D-037.)* Build a **headless pacing simulation**: AI commanders playing each other at high speed with no renderer. Use it to tune the timers before the content is built.

## RK-03: MVP scope

The agreed MVP contains:

- a server with persistence and networking;
- a real-time simulation with economy, building, fleets, combat, shields and the full siege lifecycle;
- travel between systems, with convoys and blockades;
- an always-on Deputy with a doctrine editor and an away report;
- NPC raiders;
- **9 AI commanders** that must handle expansion, offence timed to other commanders' windows and fleet logistics across systems;
- seasons and a win condition;
- the voxel renderer and HUD;
- **SYSTEM and SECTOR views**. These are needed to command travel, and phase 1 explicitly leaves them undesigned (phase-1 doc 07, item 7).

Competent strategic RTS AI is the hardest single item on that list. The MVP is not playable without it.

**Recommendation:** R-05 says "*up to* 9" AI players. Build the AI stack for 9, but set the first MVP acceptance at 3 AI commanders plus NPC raiders on a small map (one sector). Cut vacation and derelict states from the MVP, since they do nothing with one human. Add the strategic map views to the design backlog as their own document.

## RK-04: Sequencing

D-028 builds phase 1 first. I recommended that. On reflection it under-weights risk. The renderer is the most familiar territory for this team; the risky parts are the Deputy and AI behaviour, conquest pacing (RK-02), and running a persistent simulation. Building phase 1 first leaves all of them untested for months.

**Recommendation:** keep phase 1 as the first *client* deliverable, and add a parallel **headless simulation track**: a `nomad-sim` crate with a minimal economy, fleets, the siege lifecycle and a first Deputy. It runs accelerated with no graphics. It doubles as the pacing tool for RK-02, and it is the seed of the server.

## RK-05: No information model

Three decisions assume an information model that does not exist yet:

- **D-027:** AI commanders play with "the same information" as humans.
- **The phase-1 HUD:** it shows sensor contacts ("TACTICAL / SENSOR · PULSE · 40 KM") and hostile tracks.
- **D-014:** PvP depends on what each side can see.

The server's interest management (C-02), the minimap, the scouting value of ships and the meaning of "announced" Reinforced timers all depend on the model too. If it is left undefined, the server will default to sending everything, and switching to fog of war later means rebuilding the protocol.

**Recommendation:** fog of war per commander, based on sensor coverage. Holdings, ships and gates carry sensor ranges. Strategic views show *last-known* information with its age. The server sends each client only what that commander can see. Write this into doc 15, or make it a separate document.

## RK-06: Crippled instead of eliminated

D-017 says a fallen mother station emergency-jumps and "loses that site's other modules". That leaves four gaps:

- It does not say whether the attacker **captures** those modules or they are **destroyed**.
- It does not say where "a safe system" is.
- Nothing prevents repeated sieges on the same commander.
- There is no comeback path.

If those modules are captured, sieging a mother station is profitable every time it happens, which invites farming the weakest commander.

**Recommendation:**

- A fallen mother station's site modules are **destroyed**, not captured; the attacker gains the cleared site.
- The jump goes to the commander's nearest owned holding or, failing that, a free site in a low-density sector.
- After a jump, the station gets 72 h of protection: it cannot enter Reinforced.
- A comeback grant is scaled to how far the commander has fallen behind.

## RK-07: Season resets and builders

The look is a station diorama, and the pleasure of building it up is central. Seasons (D-020) wipe it every 8–12 weeks, and builder-type players may not come back.

**Recommendation:** carry over **blueprints** (saved station layouts), **doctrine presets** and **cosmetics**. Do not carry over resources, holdings or fleets. Start-of-season placement then becomes quick for returning players without giving them an advantage in power.

## RK-08 to RK-11: Rules to write down

These do not need owner decisions, only rules in the named documents.

- **RK-08** (doc 10): relief within the 12 h notice only works if fleets can reach the siege in time. Rule: any trip within a sector takes less than 6 h.
- **RK-09** (doc 15): every holding has a core module that projects at least a minimal dome; a claim without a core is an *outpost* and follows the peripheral rules. A newly captured holding gets 48 h of protection against an immediate counter-siege.
- **RK-10** (doc 12 addendum):
  - A direct order overrides doctrine for the units it covers until the player releases them, or until the player disconnects and the grace period expires.
  - The doctrine's retreat threshold applies during a direct order only if the player opts in for that order.
  - The HUD shows which units are under direct control.
- **RK-11** (doc 10): late joiners start in a reserved frontier sector with 7 days of protection.
