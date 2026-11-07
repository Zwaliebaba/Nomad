# C7 input: view-model inventory for the β boards

Status: **Agreed (round 19).** Q244–Q254 were decided as D-264 to D-274, all as recommended; §4's additions go into N4 §4.5 with those decisions. Written from the β client UI boards (TACTICAL HUD, compact HUD, SYSTEM, SECTOR, build drawer, economy drawer, start screen, resume screen) against the code at `b1a3b17`. It traces every value those boards show to where it comes from, and lists what N4 §4.5 must add or change so C7 can build its store against a fixed contract. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions or the code, and *Proposed* means it needs the owner's approval.

## 1. Method

Each value on a board gets one source code:

| Code | Meaning |
|---|---|
| **N4** | Named in N4 §4.5 as written; C7 can rely on it. |
| **CV** | Exists in `CommanderView` or a sim view type, but N4 §4.5 does not list it. Extraction work only. |
| **CL** | The client derives it from data already sent, the rules and the season clock. No protocol change. |
| **PKG** | Depends on a package not yet built (A1, U5); added with that package (D-184's rule for later fields). |
| **GAP** | Nothing produces it today, or no decision covers it. Needs a question or a correction. |

Paths such as `economy.pool` refer to `CommanderView` queries and the sim types they return (`crates/nomad-sim/src/sensors/view.rs`).

## 2. Findings

*Derived* unless a question number is given.

1. **The exposure window is per commander, not per holding.** N4 §4.5 puts "exposure window (public, S6)" on the holding entry. The sim keeps one `ExposureWindow` on `CommanderRecord`, and `CommanderView::exposure_window(commander)` is public (D-075). The view needs public **commander entries** (identity, display name, window) and an own **record entry** (`chosen_window`, `free_choice_until`, `grace_until`). This also answers the open question from the boards: rival windows are public and SYSTEM may show them.
2. **Sensed fleets carry no orders.** The phase-1 HUD shows hostile tracks as "STN-KEEP", "FORMATION" and "ATTACKING". Orders are private (D-075; `SensedFleet` has none), so the HUD may only show what can be observed: moving or stationary from velocity, closing or opening relative to a holding, and firing from N4's projectile events (D-185). The boards are corrected (§6).
3. **The topology has no typed entry.** N4 §4.5 types fleets, ships, holdings, last-known entries, lane traffic and the economy, but not systems, sites, gates and lanes, which SYSTEM and SECTOR draw from. The galaxy is fully charted and fixed for the season (D-044), so it can go once per session (Q246).
4. **The client has no rules.** Module and ship costs, build times, power draws, command costs, slot eligibility, sensor ranges, ability cooldowns and the window drift all come from the rules file, which lives in `nomad-sim`. `nomad-client` does not depend on `nomad-sim`, and a Universe can carry tuned rules (`create --rules`, D-104), so the embedded file is not enough (Q245).
5. **Nothing has a name.** `Galaxy`, `PublicHolding`, `FleetView` and the commander roster carry ids only. Doc 10 §9's naming scheme is still *Proposed*. Every board label (Kestrel-7, KR-114, VANTH, CAP WING) has no source (Q244, Q247).
6. **Build progress does not follow D-230.** `QueueItemView.building` and `ModuleView.building_ms` give milliseconds left. D-230 says the client receives the progress, the current rate and an estimated completion tick, recomputed when the rate changes. Doc 20 §5 forbids countdowns on the wire. The own queue and module entries must carry `progress`, `rate` and `eta: Tick` instead.
7. **Per-holding income does not exist.** `CommanderEconomy.income_last_hour` is commander-wide. The economy drawer's income table, and a buffer's "full in", need each holding's output after yield and power share (Q248).
8. **"Full in 52 min" would mislead.** `income_last_hour` counts production, including production that sits in remote buffers. The pool fills at the rate of delivery, which is lower while convoys are out (Q249).
9. **The route preview needs the sim's route search.** A jump's route is the shortest by time, computed in `nomad-sim/src/fleet/route.rs`. A preview that draws a different route from the one the fleet then flies is worse than none (Q250).
10. **The convoy route is invisible.** `EconomyCommand::Convoy` sets a MULE fleet's loop, but `FleetView` does not return it, so the economy drawer cannot show it. The own fleet entry needs `convoy: Vec<HoldingId>` (CV once the sim exposes it).
11. **Alerts have no end.** `Notification` is a point event. The banner shows a standing condition ("RAID · SECTOR E 38%") and needs to know when it is over (Q251).
12. **The start screen has no API.** D-245 has the server own the start screen, but nothing serves a list of Universes, create, resume or shut down to a browser that has no session yet (Q252).
13. **Queue reordering has no command.** Doc 17 §6 lists reordering; `EconomyCommand` has `Build` and `Cancel` only (Q253).
14. **STOP has no order.** The fleet card shows STOP; `FleetOrder` has no stop (Q254).
15. **The catch-up screen cannot show events.** During catch-up the session receives only `CatchUp` (N4 §4.3), so "raids repelled so far" has no source until U5's away report. The resume board is corrected (§6).

## 3. Inventory

### 3.1 Session, time, server

| Value (board) | Source | Code |
|---|---|---|
| Season day, local time (top bar) | `Welcome.clock` + tick; `final_tick` for "/ 56" | N4 (clock), CV (`final_tick`) |
| Server live, 10 Hz | session state, `CatchUp` absent | CL |
| Clock in sync (D-245) | host clock against `Welcome.clock` and delta ticks | CL |
| Catch-up progress, from/now/to, time left | `CatchUpProgress { reached, target }` + clock; rate from successive messages | N4 |
| Play enabled | first `ViewSnapshot` after catch-up | N4 |

### 3.2 Topology and names

| Value | Source | Code |
|---|---|---|
| Systems: kind, sector | `galaxy().System` | GAP (no typed entry, Q246) |
| Sites: position, deposits (kind, richness) | `galaxy().Site` | GAP (Q246) |
| Gates, lanes, lane transit time | `galaxy().Gate`, `Lane.transit` | GAP (Q246) |
| Orbits, belt (SYSTEM) | site radius from the star; flavour only | CL |
| System, site, sector names | none; doc 10 §9 *Proposed* | GAP (Q244) |
| Commander names (VANTH, YOU) | none | GAP (Q244) |
| Fleet names (CAP WING) | none | GAP (Q247) |
| Module labels (FA-1, SOL-1), slot labels (I-3) | slot index + module kind | CL |
| Control group badges 1–9 | client state (doc 17 §7) | CL |

### 3.3 Own economy

| Value | Source | Code |
|---|---|---|
| Alloy, crystal: pool, cap | `economy.pool`, `.cap` | N4 |
| Income per hour | `economy.income_last_hour` | N4 |
| Full in … | (cap − pool) ÷ delivery rate | GAP (Q249) |
| Cap breakdown | rules + own storage modules | CL (needs rules, Q245) |
| Command used, cap | `economy.command_used`, `.command_cap` | N4 |
| Command by class | own ships × rules command cost; queued ships count too | CL (Q245) |
| Diminishing yield, producing holdings | `economy.yield_permille`, `.producing_holdings`; F from rules | CV |
| Lost to full storage | `economy.lost` | CV |
| Core kit price | `economy.core_kit_price` | CV |
| Income per holding | none | GAP (Q248) |
| Depot, direct, buffer | `holding_economy.depot`, `.delivers_to_pool` | CV |
| Buffer, buffer cap | `holding_economy.buffer`, `.buffer_cap` | N4 ("buffers") |
| Buffer full in | buffer cap − buffer ÷ holding output | GAP (Q248) |

### 3.4 Own holdings

| Value | Source | Code |
|---|---|---|
| Holding is the mother station | `Holding.mother_station` (not in `PublicHolding`) | GAP: add to the holding entry |
| Slots with their modules, free slots | `holding_economy.slots` | CV (N4 lists modules, not the template) |
| Module kind, hull %, disabled | `ModuleView` | N4 |
| Module and queue progress, ETA | `building_ms` today | GAP (D-230, finding 6) |
| Queue items, price, lane, shortage | `QueueItemView` (`short: Shortage`) | N4 ("queue") |
| Slot eligibility per module (catalogue) | rules / sim code | GAP (Q245) |
| Power supply, demand, per consumer | `PowerAllocation` | N4 ("power report") |
| Supply by source (SOL 620 · CORE 520) | modules × rules supply | CL (Q245) |
| Power profile | `holding_economy.profile` | CV |
| REPAIR on | `holding_economy.repairing` | CV |
| Rally fleet | `SetRally` exists; not in `HoldingEconomyView` | GAP: add `rally: Option<FleetId>` |
| Stored core kits | `holding_economy.kits` | CV |
| Dome sectors: strength, weak, breached | `defence.sectors` | N4 |
| Sector open (siege) | `SectorView.open` | CV |
| Turrets: count, powered, firing; STOP | `defence.turrets*`, `.turrets_hold` | CV |
| SHLD SURGE, FLAK SCREEN ready and active | `defence.surge_ready_at`, `.flak_screen_ready_at`, `.flak_screen_until` | CV |
| Hull (selection) | core module hp | CL |
| Docked ships, capacity | own ships with `Place::Docked`; rules capacity + dry docks | CL (Q245) |
| Reinforced, siege deadlines | `siege(id).state` (own only, D-254) | N4 (amended by D-254) |
| Breachable from | `breachable_from` (public, D-170) | CV |
| Deputy deferred decisions | A1 inbox (D-202), shown by U5 | PKG (A1, U5) |

### 3.5 Commanders and the season

| Value | Source | Code |
|---|---|---|
| Roster | `commanders()` | GAP: no commander entry (finding 1) |
| Exposure window today, closes at | `exposure_window(me)` | GAP: wrong entry in N4 (finding 1) |
| Window move rule text | rules (`window_drift_minutes_per_day`) | CL (Q245) |
| First-week window choice | `own_record.chosen_window`, `.free_choice_until` | CV |
| Rival windows (SYSTEM, optional) | `exposure_window(other)` | CV |
| Standings, threshold, near-win, result | `standings()`, `early_win_threshold()`, `near_early_win()`, `season_result()` | N4 (`standings` collection) |
| Control states | `control` collection | N4 |
| Control bar counts per system | public holdings, outposts excluded | CL |

### 3.6 Fleets

| Value | Source | Code |
|---|---|---|
| Own fleet: position, heading, velocity, mode, composition | `FleetView`, `ShipView` | N4 |
| Own fleet: order, queued orders, ROE, engaged until | `FleetView` | N4 |
| Own fleet: place (docked, in transit with arrival) | `FleetView.place` | N4 |
| Own fleet: jump stage (spool-up until) | `FleetView.travel` | CV |
| Own fleet: convoy route | none | GAP (finding 10) |
| Status tag PATROL / ENGAGED / HAULING / DOCKED | order, `engaged`, convoy, place | CL once the convoy route is sent |
| Direct control, since when | A1 marks the fleet (RK-10, A1 plan §4) | PKG (A1) |
| Doctrine role, leash; Deputy preset | A1 doctrine | PKG (A1; U4 edits it) |
| Idle fleets (chip) | own fleets in space with no order | CL |
| Sensed fleets: ships, classes, hull, position, velocity, shrouded | `SensedFleet`, `SensedShip` | N4 |
| Sensed status text | velocity, distance trend, projectile events | CL (finding 2) |
| Last-known: classes, position, seen at | `LastKnownFleet` | N4 |
| Lane traffic: from, to, gate, departed, arrives, classes seen | `LaneTraffic` | N4 |
| Contested gate (route preview) | sensed armed hostiles within 5 km of the gate (D-055) | CL |
| Order preview ETA (SYSTEM) | rules speeds and acceleration; an estimate, shown with "~" | CL (Q245) |
| Jump route and leg arrivals (SECTOR) | the sim's route search | GAP (Q250) |
| Projectiles, hits | N4 projectile events (D-185) | N4 |

### 3.7 Alerts

| Value | Source | Code |
|---|---|---|
| Toasts, browser notification | `Notification { tick, subject, kind }` | N4 |
| Standing banner and when it clears | none | GAP (Q251) |
| Banner detail (sector %, attacker class, distance) | defence + sensed entries for `subject` | CL |
| Last alert (Space) | client history of notifications | CL |

### 3.8 Start screen (outside a session)

| Value | Source | Code |
|---|---|---|
| Server build, address | server | GAP (Q252) |
| Universes: name, seed, AI count, difficulty, length, day, state, last stop | `meta` in each Universe's database (`status` command reads it) | GAP (Q252) |
| Catch-up needed on resume | last tick against now | GAP (Q252) |
| Ended season standings | `season_result` read model (D-196) | GAP (Q252) |
| Create, resume, shut down | `create` arguments as a form (D-245) | GAP (Q252) |

### 3.9 Commands the boards issue

| Control | Command | Code |
|---|---|---|
| Fleet orders: move, attack, attack-move, patrol, guard, station-keep, dock, launch, retreat, jump; ROE | `Command::Fleet` with `FleetOrder` | exists |
| STOP (fleet card) | none | GAP (Q254) |
| Holding: launch ready, SHLD SURGE, FLAK SCREEN, turrets hold | `Command::Holding` | exists |
| PWR ROUTE, REPAIR, RALLY | `EconomyCommand::SetPowerProfile`, `SetRepair`, `SetRally` | exists |
| Queue a module or ship, cancel | `EconomyCommand::Build`, `Cancel` | exists |
| Reorder the queue | none | GAP (Q253) |
| Blueprints: save, apply | none; the type is shared with the AI (D-215) | PKG (U3) |
| Create, split, merge fleets | `CreateFleet`, `MergeFleets` | exists |
| Hand back to the Deputy | A1 release command | PKG (A1) |
| Exposure window choice | `SetExposureWindow` | exists |
| Focus (rate tier) | `SetFocus` (D-186) | N4 |
| Name a fleet | none | GAP (Q247) |

## 4. What N4 §4.5 should add or change

*Derived* from §2 and §3, except where a question decides it.

- **Commander entries** (public): id, display name (Q244), exposure window `{ start, opens, closes }`. **Own record** (own): `chosen_window`, `free_choice_until`, `grace_until`. Remove the window from the holding entry.
- **Topology** (public, once per session): per Q246.
- **Rules** (public, once per session): per Q245.
- **Holding entry:** `mother_station: bool`. For own holdings also: the slot template with modules, `profile`, `repairing`, `rally`, `kits`, `depot`, `delivers_to_pool`, `yield_permille`, per-holding output (Q248), the defence's `open`, turrets, `turrets_hold`, ability ready and active ticks, and `breachable_from`.
- **Progress fields** for queue items and modules under construction: `progress`, `rate`, `eta: Tick` (D-230), never milliseconds left.
- **Own economy:** add `yield_permille`, `producing_holdings`, `lost`, `core_kit_price`, and delivery over the last hour (Q249).
- **Own fleet entry:** add `travel` (spool-up until) and `convoy`; `direct_control` and `doctrine_role` arrive with A1.
- **Alerts:** per Q251.

## 5. Questions

Numbered Q244–Q254 (round 19). Each has a recommendation and alternatives.

- **✅ Q244 (D-264): Names of systems, sites, sectors and commanders.** Doc 10 §9 is *Proposed* and nothing in the code names anything.
  *Recommendation:* agree doc 10 §9 and implement it as a pure function of seed and id in `nomad-types`, so the client and the server name things identically without sending strings; AI commanders get names from a word list the same way; the human's name comes from `create`. *Alternatives:* the server generates names at `create` and sends them in the topology and commander entries (strings on the wire, one generator); ids only in β, names later (every board label becomes a code).

- **✅ Q245 (D-265): How the client gets the rules.** The client needs costs, times, power, slot eligibility, ranges and cooldowns; the rules types live in `nomad-sim`, which the client does not link, and a Universe can carry tuned rules (D-104).
  *Recommendation:* move the rules types (serde only, no logic) from `nomad-sim` to `nomad-types`, and send the Universe's rules once per session, after `Welcome`. *Alternatives:* the client embeds the same-commit `default.json` (wrong for tuned Universes); the client links `nomad-sim` (pulls the whole simulation into the wasm module and blurs doc 20 §3's boundary).

- **✅ Q246 (D-266): How the topology reaches the client.** N4's per-entity minimal entries carry no system kind, site position, deposits or lane transit.
  *Recommendation:* a `Topology` message once per session, after `Welcome`: systems (kind, sector), sites (position, deposits), gates (position, lanes), lanes (gates, transit, kind). The `public` collection then carries only what changes: holdings and their owners. *Alternatives:* typed topology entries inside `public` (diffed like everything else, though they never change); the client regenerates the galaxy from the seed (needs the generator in the client and ties the client to the generator's version).

- **✅ Q247 (D-267): Fleet names.** The roster shows names; no design gives fleets one.
  *Recommendation:* fleets get a player-set label, stored by the server outside the simulation (a small table beside the read models), set with a session message that the simulation never sees; no label means "FLEET n". Not gameplay, so not journaled; survives a new browser and works for MP-1. *Alternatives:* a label in the simulation, set by a `Command` and journaled (simple, but puts cosmetic data into the deterministic state); labels in the browser's storage only (lost with the browser, invisible to the native build).

- **✅ Q248 (D-268): Per-holding output.** The income table and a buffer's "full in" need it.
  *Recommendation:* the sim computes each own holding's output per hour after yield and power share at the last economy tick, and the own holding entry carries it. *Alternatives:* the client estimates it from rules and modules (wrong whenever power is short, which is when the player looks); per-holding production over the last hour (accurate, but lags a change by an hour).

- **✅ Q249 (D-269): The pool's fill estimate.** `income_last_hour` counts production, not delivery.
  *Recommendation:* add `delivered_last_hour` to the own economy and base "full in" on it. *Alternatives:* the client measures the pool's change over its own session (fails right after login); keep `income_last_hour` and accept an optimistic estimate.

- **✅ Q250 (D-270): Route preview.** The jump route is the sim's shortest-by-time search.
  *Recommendation:* move the route search (pure, over the topology and the fleet's cruise speed) to `nomad-types` beside Q245's rules, so the preview runs the same code as the fleet. *Alternatives:* a request–reply "preview route" message answered by the server (a round trip on every hover, and a new message kind outside commands); a client-side search written separately (drifts from the sim).

- **✅ Q251 (D-271): Alert lifecycle.** The banner needs a start and an end; `Notification` has only a start.
  *Recommendation:* own active alerts as view state: an `alerts` collection of `{ id, kind, subject, since }`, removed when the condition ends (raid over, sector back to full, siege resolved); `Notification` stays the trigger for toasts. *Alternatives:* the client clears the banner after a fixed time or when the player looks at the subject (no server work, can hide a live raid); the server sends an "alert ended" notification (two event kinds to keep in step).

- **✅ Q252 (D-272): The start screen's API.** No session exists before a Universe runs.
  *Recommendation:* a small JSON API on the same `axum` server (D-243), under the same Host and Origin guard (D-244): list Universes from their `meta`, create, resume, shut down, and the final standings of an ended season. The page loads it before the game connects. *Alternatives:* lobby messages on the WebSocket before `Hello` (one transport, but a second protocol in the frozen prefix's space); start-screen commands in `nomad-protocol` after a session to a "no Universe" server (sessions are per commander, which has none yet).

- **✅ Q253 (D-273): Reordering the build queue.** Doc 17 §6 lists it; no command exists.
  *Recommendation:* `EconomyCommand::Move { holding, item, before }` for items not yet started; started items keep their place. *Alternatives:* drop reordering from β and keep cancel and re-queue (cancel is free before the start, D-068).

- **✅ Q254 (D-274): STOP on the fleet card.** `FleetOrder` has no stop.
  *Recommendation:* STOP sends `StationKeep` at the fleet's current position with `Queueing::Replace`; no protocol change. *Alternatives:* a `Stop` order that clears the queue and coasts to a halt (a new rule); no STOP on the card.

## 6. Board corrections made with this inventory

- **TACTICAL:** hostile tracks show observable state (STATIONARY, FIRING), and the banner reads "HVY CRUISER STATIONARY AT 3 KM" (finding 2). Docked ships read 1 / 30 (mother core 24 plus the dry dock's 6, D-102), since only the WARDEN is docked. RALLY shows the fleet new ships join (`SetRally` takes a fleet, not a point).
- **SYSTEM:** DOCK shows 29 free.
- **Economy:** the line about lane history is gone; lane traffic is kept for only 10 minutes after arrival (`lane_traffic_after_arrival_s`). The convoy route is shown instead.
- **Resume:** the "so far" counts are gone (finding 15).

## 7. Effect on other documents

- **N4 plan §4.5:** the additions of §4; the exposure window moves to commander entries.
- **Doc 10 §9:** naming becomes agreed or stays proposed (Q244).
- **Doc 20 §3:** rules types and route search move to `nomad-types` if Q245 and Q250 are accepted.
- **Doc 21 §4.3:** the start screen's API (Q252).
- **`02-open-questions.md`:** round 19 lists Q244–Q254 once the owner has seen them.
