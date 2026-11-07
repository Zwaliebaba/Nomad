# Work package S6 — Offline and siege rules

Status: **Built** (Q142–Q160 → D-154–D-172). S6a (tasks 1–4) is built in `crates/nomad-sim/src/commander/` and `announce.rs`, the server's `create --window` and `nomad-harness --windows`; §10 lists how it was built. S6b (tasks 5–9) is built in `crates/nomad-sim/src/siege/`, with D-224, D-228 and D-229, and `nomad-harness --siege`; §11 lists how it was built. Builds on S3 (fleets, orders, routes), S4 (modules, outposts, pool, command cap) and S5b (dome, breach event, `immune` and `open` switches, damage ledger, module damage). This plan is also the brief for whoever builds S6. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Goal

A holding's core can only be lost inside its owner's exposure window, after at least 12 hours' notice, in a scheduled 60-minute siege that needs an online attacker (D-015). S6 makes that true in the simulation:
- exposure windows derived from activity;
- the shield lifecycle from breach to Held or Fallen;
- capture and the emergency jump;
- the protections that follow each outcome;
- the window rule for peripheral structures;
- the lapse of direct orders after a disconnect.

Everything runs on deadline ticks, replays exactly, and is announced as data that S7, N4 and the UI carry. The harness can drive a siege end to end from scripted commands, which MVP-α acceptance requires (doc 30 §3).

## 2. Inputs

- **Offline rules:** doc 12 in full:
  - PR-1 to PR-5 and the stakes rule (§3);
  - the shield lifecycle (§4);
  - the connection rules (§5.3);
  - the defaults (§8).
  - The agreed decisions behind it: D-013, D-015, D-016, D-017, D-032, D-033, D-034, D-057, D-138, D-149. Vacation and derelict states are outside the MVP (D-038).
- **Combat side:**
  - doc 15 §5.1 (dome, breach, Reinforced immunity, open sector), §6 (siege mechanics), §7 (capture and emergency jump), §8 (claims and outposts; D-054), §9 (gates; D-055), §10 (raiders never breach; D-056);
  - D-140 to D-142 (sectors and regeneration), D-147 (SHLD SURGE).
- **The S4 and S5 plans:** S4 owns claiming and outposts (D-135); S5b emits `SectorBreached`, honours `immune` and `open(sector)`, and keeps the damage ledger (S5 plan §4.4–§4.6).
- **Time:** doc 11:
  - §2: Universe time is UTC computed from the tick;
  - §3: deadline ticks in a min-heap, calendar deadlines converted when created;
  - §4.1: timers in phase 2, combat in phase 7, activity sampling every 60 s in phase 9;
  - §6: connect and disconnect are journaled inputs;
  - §7 and D-047: catch-up with every commander offline.
  - Also D-109, D-116, D-117.
- **Information:** doc 16 §2. The window is public (D-075); online status is private (D-075). Siege announcements go to the owner and the commanders with breach credit.
- **AI:**
  - doc 13 §4: the Deputy's alert, abilities and evacuation;
  - doc 13 §5: the Strategist's *Defend*, *Breach and siege* and *Retake* goals, and the siege timing rule;
  - doc 13 §6 and D-080: AI schedules are seeded and not journaled;
  - D-077: one validator, and AI reads only `CommanderView`.
- **Code:**
  - `crates/nomad-sim`: `UniverseState` (`connected`, `catching_up`, `holdings`, `timers`, `transits`), `TimerQueue`, `SeasonClock`, the rules file, the validator, `TickOutputs`, snapshot format 6.
  - `crates/nomad-server`: catch-up disconnects, the season-end freeze.

## 3. Scope

**In scope:**

- **Commander records** in the simulation: kind (human or AI), presence, exposure window and activity history.
- **Activity sampling** and the **activity-derived window** (D-032), including the free choice in a commander's first 7 days.
- **Presence:**
  - a human is online while connected (journaled inputs);
  - an AI is online by its schedule (A5), or by a harness operator input until A5 exists;
  - **the 120 s disconnect grace** and the lapse of direct orders (doc 12 §5.3).
- **The holding defence state machine:**
  - Nominal;
  - Reinforced, with its exit slot (D-015, D-033);
  - Siege;
  - Held;
  - Fallen.
- **Capture** of a normal holding and the **emergency jump** of a fallen mother station, of docked ships after a capture, and the comeback grant (D-017, D-034, D-057).
- **Protections:** 48 h after Held, 48 h for a new owner, 72 h after an emergency jump.
- **The window rule** for peripheral structures (doc 12 §3, D-138): the `structure_outcome_at_zero` hook that S5 leaves.
- **The outpost combat exception** (D-054): outposts under construction can be destroyed at any time.
- **The contested-gate flag** (D-055), if S4 has not already added it for convoys.
- **Announcements** and the events the away report needs.
- **Read-only views** for the Deputy and the Strategist.
- **Harness:** a scripted-siege mode, and a property test of the stakes rule.

**Out of scope:**

- Detection and fog of war (S7). S6 announces to fixed recipient lists; S7 and N4 decide what else a commander sees.
- The Deputy's doctrine behaviour (A1) and the Strategist's siege planning (A4).
- AI schedules (A5).
- Alliances (after the MVP, D-040).
- Vacation and derelict states (D-038).
- Late joiners (outside the MVP).
- Notifications outside the client (doc 30 §6).

## 4. Design

### 4.1 Commanders, presence and the disconnect grace

*Derived* from doc 11 §6, doc 12 §5.3 and D-080; record layout *Proposed*.

- `UniverseState` gains `commanders: BTreeMap<CommanderId, Commander>`, created at Universe creation from the roster. The server already knows which commander is human (`HUMAN_COMMANDER`). Each record holds:
  - `kind`;
  - `presence` (online, or offline since a given tick);
  - the exposure window;
  - the activity history;
  - pending Reinforced exits by window.
- **Humans:** online between the journaled `Connected` and `Disconnected` inputs. The server already disconnects everyone at catch-up start, and N3 at server start.
- **AI:** online by its seeded schedule (D-080), supplied by A5. Until A5, the operator input `SetPresence { commander, online }` sets it. It is accepted for AI commanders only and is journaled like any operator input, so harness runs replay. A production server never sends it.
- **Grace:** a commander who goes offline keeps their direct orders for 120 s, a deadline tick (Q150). After that the orders lapse (§4.2).
- **"Online" for siege rules** includes the grace period (Q148).

### 4.2 Lapse of direct orders

*Agreed* rule (doc 12 §5.3, PR-3); scope of the lapse is Q150.

- When the grace deadline fires and the commander is still offline, every fleet of theirs drops its direct orders and falls back to its fleet doctrine.
- Until A1 exists, "doctrine" means station-keep where the fleet is.
- Economy queues, convoy routes and the replacement list are doctrine, not direct orders, so they continue (doc 12 §5.2).
- A reconnect before the deadline cancels it.
- The same rule applies to AI commanders going offline by schedule. The Strategist writes its doctrine first (doc 13 §6).

### 4.3 Activity and the exposure window

*Agreed* rule (D-032: the most active 4 h over a rolling 14 days, drifting by at most 1 h per day, free choice in the first 7 days). Sampling and computation are *Proposed* (Q142–Q145, Q159).

- **Time of day** is computed from Universe time in UTC: `(season_start_unix_ms + tick × 100) mod 86 400 000`, in integer milliseconds. Windows are UTC ranges, as doc 11 §2 says.
- **Sampling:** every 60 s, staggered per commander (doc 11 §4.1 phase 9), each commander's presence adds one minute to the current quarter-hour bucket of the current UTC day.
  - The history is 14 days × 96 buckets of `u8` (0–15 minutes), about 1.3 KB per commander, kept in the snapshot.
  - Days older than 14 are dropped.
  - Catch-up minutes count as offline (Q159).
- **The window** is 16 contiguous buckets (4 h), wrapping round midnight, stored as a start bucket.
  - It moves only when it closes, at its end deadline (Q143), so a window never changes while it is open.
  - The target is the 4 h start with the largest summed activity.
  - The window moves towards it by at most 4 buckets (1 h) per day, the shorter way round.
  - Ties keep the current start, then prefer the nearest start, then the earliest.
- **Freeze:** the window does not move while any of the commander's holdings is Reinforced or under siege (Q144), so a known attack cannot be dodged by drift.
- **Initial window:** set at creation. The human's comes from `create --window`, by default the client's local 19:00–23:00. An AI's comes from its schedule (A5), otherwise a harness default.
  - During the first 7 days, a `SetExposureWindow` command may change it.
  - The change takes effect when the window is next closed (Q145).
- **Public:** every commander's window start is public (D-075), so it is part of every `CommanderView`.

### 4.4 The holding defence state machine

*Agreed* lifecycle (doc 12 §4, D-015, D-033, D-057); details marked.

```text
Nominal ──breach──► Reinforced{exit} ──exit, attacker online──► Siege{end}
                        │                                        │      │
                        └──exit, no attacker online──► Held ◄─clock─┘      core 0
                                                        │                   ▼
                              protection 48 h ◄─────────┘                Fallen
                                                                 capture or emergency jump
```

- **Breach** *(Agreed, D-015; edge rule Q147):* S5b emits `SectorBreached { holding, sector, tick }` when a hostile hit takes a sector to 0. S6 acts on it only when the holding is breachable: Nominal and not protected. Then S6:
  - reads the **attackers** from the ledger: every commander whose ships damaged this dome in the 10 min before the breach (D-057);
  - sets `immune`;
  - computes the exit (below);
  - announces the breach.
  - NPC raiders do no dome damage (D-056), so they never breach and never get credit.
- **Exit time** *(Agreed, D-015 and D-033; slot rule Q146):* the earliest slot at least 12 h after the breach and later than the exits of the commander's earlier-breached pending holdings.
  - A slot is `window start + k × 60 min`, k = 0–3, so at most 4 per window.
  - Later breaches roll over to later windows.
  - The exit uses the owner's window as it is at the breach. It is fixed and announced at once, and the freeze (§4.3) keeps the real window from drifting away from it.
  - The calendar time becomes a deadline tick when it is computed (doc 11 §3).
  - A breach inside the window still waits for a later window (doc 12 §4).
- **Reinforced:** the dome takes no damage (`immune`). Ships launch and dock, and production continues (doc 12 §4).
- **Siege start** *(Agreed, D-057):* at the exit deadline:
  - if no attacker is online (§4.1), the result is **Held** at once;
  - otherwise the breached sector is `open` (no regeneration, no blocking) and the other sectors behave normally;
  - modules inside the dome become damageable through the open arc (doc 15 §6);
  - the siege clock ends 60 min later (36 000 ticks).
  - During catch-up nobody is online, so a siege that falls due then is Held (doc 11 §7, D-047).
- **During the siege:**
  - The ledger counts core damage and total damage per commander.
  - Who may damage the core and who may capture is Q151.
  - An attacker who goes offline loses their direct orders after the grace (§4.2). Doctrine never attacks, so their assault stops, but the clock runs on (Q149).
- **Held** *(Agreed, D-057, D-015):* when the clock ends with the core standing:
  - every sector restores to 50%, and `open` and `immune` clear;
  - the holding cannot be breached for 48 h.
  - Phase order decides a tie: timers (phase 2) run before combat (phase 7). On the end tick the siege is already over, so core damage dealt in that tick does not count. *Derived* from doc 11 §4.1.
- **Fallen** *(Agreed, D-057, D-017):* the core reaches 0 hull during the siege. The holding is captured (§4.5), or a mother station emergency-jumps (§4.6).
- **Protection** *(Agreed, D-015, D-034, D-057; behaviour Q147):*
  - covers 48 h after Held, 48 h for a capturer, and 72 h after an emergency jump;
  - is stored as one `breachable_from` tick per holding;
  - lets sectors still be worn down and lets peripheral structures be disabled, but triggers no breach.
- **Weakened** is display only: a sector below 50% (phase 1). It is not a state.
- **Season end:** the server freezes the Universe at the final tick (D-116), so an exit or siege scheduled after it never happens. *Derived.*

### 4.5 Capture of a normal holding

*Agreed* (D-057, doc 15 §7); details Q151, Q156.

- **The capturer** is the commander with the most core damage during the siege. Ties go to the larger total damage to the holding, then to the lower `CommanderId`.
- **The holding:**
  - changes owner;
  - the core restores to 25% hull and the sectors to 25%;
  - surviving modules transfer;
  - build queues are cancelled (Q156);
  - the command caps of both commanders update (D-129).
- **The old owner's docked ships** emergency-jump to the old owner's nearest holding (§4.6). The old owner always has one, because the mother station is never captured (D-017).
- **The new owner** gets 48 h of protection.
- **Ownership** is public (D-044), so the capture is announced to everyone.

### 4.6 Emergency jumps and the mother station

*Agreed* (D-017, D-034, D-057); destination and travel Q152–Q155.

- **An emergency jump** is an uninterruptible transit along the fastest route (S3 `route`), taking that route's time. Throughout, the ship or station cannot be targeted and is invisible, like lane transit (D-042, D-074). On arrival it docks if there is capacity, and otherwise station-keeps by the destination's core (Q154).
- **Fallen mother station:**
  - The site's other modules are destroyed.
  - The core and its docked ships jump to the destination (Q152, Q153).
  - At the destination the core becomes a holding with a mother-station core: 24 docking places (D-102) and a command cap base of 120 (D-129). It has a minimal dome and 72 h of protection.
  - The commander receives the comeback grant (Q155).
  - While the station is in transit it stays the commander's mother station for the command cap and for its jumped ships. It is not a holding for control (D-041).
- **The cleared site** passes to the capturer as an outpost with a core under construction (D-057, doc 15 §7), through S4's outpost API, at no kit cost. It can be destroyed at any time (D-054).

### 4.7 Peripheral structures, outposts and gates

*Agreed* (doc 12 §3, D-138, D-149, D-054, D-055); disabled structures in the window are Q157.

- **`structure_outcome_at_zero`** (the S5 hook):
  - a peripheral structure (solar array, harvester, extractor) at 0 hull is **destroyed** if the tick lies inside its owner's window, and **disabled** otherwise;
  - an inner module at 0 hull during a siege is destroyed (doc 15 §6);
  - an outpost under construction is destroyed at any time (D-054).
- **Recovery** of a disabled structure follows D-149: online again after 10 min with no hostile in the holding's 20 km ops area, at 10% hull.
- **Contested gates** (D-055): armed hostile ships station-keeping within 5 km of a gate make it contested for convoys. S4 consumes the flag. S6 adds it as a derived per-gate flag at 1 Hz if S4 has not already done so. The rest of D-055 (scatter, shroud, spool-up) is in S3.

### 4.8 Announcements, the away report and AI views

*Derived* from doc 16 §2 and D-075; the shapes are *Proposed*.

- **Announcements:** `TickOutputs` gains `announcements`, each with a tick, a recipient set (named commanders, or public) and a kind. The kinds are:
  - `Breached { holding, sector, exit_tick, attackers }`: to the owner and the attackers;
  - `SiegeStarted { holding, end_tick, open_sector }`: to the owner and the attackers;
  - `Held { holding, reason, breachable_from }`: to the owner and the attackers, with the reason *clock* or *no attacker online*;
  - `Captured { holding, from, to }`: public (D-044);
  - `MotherStationJumped { from_site, to_site, arrival_tick, protected_until }`: to the owner, with the outpost public;
  - `WindowMoved { commander, start }`: public (D-075).
  - Deadlines are sent, not countdowns (doc 11 §3).
- **No event log in the state.** Everything an announcement says can be rebuilt from state and outputs, and replay reproduces the outputs. The server keeps the away report's 7-day history (D-092) as a read model fed from outputs, which is N4 and U5 work.
- **The away report's S6 content:** breaches and their exits, sieges and outcomes, captures in both directions, emergency jumps, structures disabled or destroyed and restored, window moves, and orders that lapsed.
- **Views for A1 and A2 to A4.** Read-only accessors give each holding's defence state with its deadlines, `breachable_from`, credited attackers, every commander's public window, and the commander's own presence. Doc 13 §5's siege timing rule then reduces to comparing the Strategist's online schedule with an exit tick.

### 4.9 Rules file, snapshots and performance

- **Rules file** (D-104): a `sieges` section holds:
  - notice 12 h, siege clock 60 min, Held immunity 48 h, capture protection 48 h, jump protection 72 h (D-015, D-034, D-057);
  - grace 120 s and the 10 min recovery (doc 12 §8);
  - breach credit 10 min (D-057);
  - window 4 h, drift 1 h per day, history 14 days, free choice 7 days (D-032);
  - stagger 60 min, 4 per window (D-033);
  - Held restore 50%, capture restore 25% and 25% (D-057).
  - All are configurable, as D-015 says.
- **Snapshots:** S6a bumps the format once (commander records and order-lapse deadlines), and S6b once (defence states, slots, protections, emergency jumps). Pins are re-recorded each time.
- **Performance:** event-driven except sampling, which is one bucket increment per commander per minute, and window moves, about 1 500 additions per commander per day. Both are negligible at stage 2 against doc 11 §4.3.

## 5. Timeline check

*Derived* from the agreed values; the AI behaviour is assumed.

**A siege against an offline defender.** Defender D (human) has a window of 19:00–23:00 UTC. Attacker A (AI) has sessions at 08:00–09:30 and 18:00–20:00.

| Universe time | Event | Rule |
|---|---|---|
| Day 1 14:20 | A's fleet takes sector E of D's holding H2 to 0. A has breach credit. H2 is Reinforced and immune. | D-015, D-057 |
| Day 1 14:20 | Exit computed. Day 1 19:00 is only 4 h 40 min away, so it doesn't qualify. Day 2 19:00, slot 0, is 28 h 40 min away. Announced to D and A. | D-015, D-033 |
| Day 1–2 | D stays offline. D's window is frozen while H2 is Reinforced. D's Deputy repairs and keeps the replacement list. | Q144, D-016 |
| Day 2 19:00 | A is online (session 18:00–20:00), so the siege starts. Sector E is open and the clock ends at 20:00. | D-057 |
| Day 2 19:00–19:16 | Without turrets, the core dies in about 13 min (S5 plan §5). With D's Deputy using turrets and FLAK SCREEN, it takes longer. Here it dies at 19:16. | D-058 |
| Day 2 19:16 | Fallen. A captures: core at 25%, sectors at 25%, modules transfer, queues cancelled. D's docked ships emergency-jump to D's mother station. A is protected until day 4 19:16. | D-057 |

The core was lost inside D's window, 28 h 56 min after the breach, while D was offline. That is exactly what the stakes rule allows (D-015). The notice always lies between 12 h and 36 h, plus up to 3 h of stagger, plus whole days for rollovers.

**The same siege with the PC off from day 2 18:00 to day 3 08:00.** At the next start the server catches up with everyone offline (D-047). At 19:00 no attacker is online, so H2 is Held and protected until day 4 19:00.

On the local MVP, a switched-off PC therefore protects every holding, the AI's included, while it is off. The AI windows then drift towards the hours the PC is on (Q159). That self-corrects within days, but it is a risk for AI-against-AI sieges in the 14-day playtest (§9).

**Stagger.** D's holdings H2–H6 are breached between 10:00 and 11:00 on day 1. Day 1 19:00 is less than 12 h away, so the slots on day 2 are H2 19:00, H3 20:00, H4 21:00 and H5 22:00. H6 rolls over to day 3 19:00. The 22:00 siege ends at 23:00, the end of the window (D-033).

## 6. Questions

Numbered Q142–Q160 (Q161–Q165 unused). Each has a recommendation and alternatives.

- **✅ Q142 (D-154) — What counts as activity (D-032).** Doc 11 §6 drives the window from connect and disconnect events, but a client left open overnight inflates activity.
  *Recommendation:* a minute counts when the commander is online at the 60 s sample (connected, or scheduled online for an AI). An idle open client counts. The 1 h/day drift and the 14-day history limit the effect, and a window in hours when the player isn't there only hurts that player. *Alternatives:* online and at least one command in the last 10 min; commands only.
- **✅ Q143 (D-155) — Window granularity and when it moves.** *Recommendation:* 15-minute buckets over 14 UTC days. The window moves only at its own end, never while open, towards the best 4 h by at most 1 h per day. Ties keep the current window, then prefer the nearest start, then the earliest. *Alternatives:* recompute every commander at 00:00 UTC; 1-minute granularity.
- **✅ Q144 (D-156) — Freeze the window during a pending siege.** The exit is fixed at the breach, but the real window keeps drifting, so a siege could start up to a couple of hours outside the defender's current window. *Recommendation:* the window does not move while any of the commander's holdings is Reinforced or under siege, as D-032 freezes history during vacation. *Alternatives:* let it drift (the exit stays fixed anyway); freeze the activity history instead of the window.
- **✅ Q145 (D-157) — The initial window and the first 7 days.** *Recommendation:* set at creation: the human's from `create --window`, by default the client's local 19:00–23:00; an AI's from its schedule. A `SetExposureWindow` command may change it during the first 7 days, taking effect when the window is next closed. After that only activity moves it. *Alternatives:* start from a default and let activity alone move it; allow one change per day in the first 7 days.
- **✅ Q146 (D-158) — Stagger slots and the 12 h notice (D-015 against D-033).** D-015 says the exit is "the start of the first window that begins at least 12 h after the breach"; D-033 spaces several exits 60 min apart. If the window starts 11.5 h after a second breach, the strict reading skips a whole day, although slot 1 would give 12.5 h of notice. *Recommendation:* the exit is the earliest slot (window start + 0, 1, 2 or 3 h) at least 12 h after the breach and after the commander's earlier-breached exits. *Alternatives:* the strict reading (the window itself must start 12 h later, then the next free slot in it).
- **✅ Q147 (D-159) — A sector at 0 during protection (doc 12 §3 against the protection rules).** Doc 12 says a sector at 0 triggers Reinforced, but during the 48 h or 72 h protections no Reinforced is allowed, and nothing says what happens instead. *Recommendation:* sectors can still be worn to 0, keep blocking and regenerate, and no breach happens. Once protection ends, the next hostile hit on a sector at 0 is a breach. *Alternatives:* protection makes the dome immune to all damage; a sector still at 0 when protection ends breaches at once.
- **✅ Q148 (D-160) — Who counts as online at the siege start.** *Recommendation:* connected or inside the 120 s grace; an AI by its schedule. A network blip at the start doesn't hand the defender a free Held. *Alternatives:* connected only; online at any moment in the first 5 min of the siege.
- **✅ Q149 (D-161) — Attackers who go offline during the siege.** *Recommendation:* the clock runs on. Their direct orders lapse after the grace, and doctrine never attacks a core, so their assault stops in practice. *Alternatives:* the siege ends Held once no attacker has been online for 5 min; the clock pauses.
- **✅ Q150 (D-162) — Which orders lapse after the grace (doc 12 §5.3, PR-3).** *Recommendation:* all direct fleet orders lapse and fleets fall back to doctrine (A1; until then station-keep). Economy queues and convoy routes are doctrine and continue. *Alternatives:* only offensive orders (Attack, Attack-move) lapse; orders into systems where the commander has no holding also lapse.
- **✅ Q151 (D-163) — Who may damage the core and capture (D-057 against doc 15 §7).** D-057 says "the commander who dealt the most core damage captures"; doc 15 §7 says "the attacker who dealt the most core damage". In a free-for-all a third party can reach an open sector. *Recommendation:* any commander hostile to the owner can damage modules through the open sector, and capture goes to the most core damage among all of them. D-057's tie-breaks already deter last-second sniping. NPC raiders deal no damage to modules inside the dome, so they never take part in sieges (D-056). *Alternatives:* only commanders with breach credit can damage the core; anyone can damage it, but only credited attackers can capture.
- **✅ Q152 (D-164) — Where a fallen mother station goes (conflict in D-034).** D-034 sends it "to the commander's nearest owned holding", but a site has one core, so it cannot land on an existing holding. *Recommendation:* a free site in the system of the commander's nearest holding by route time; if there is none, the free site in the least-held claimable system (Q153); if there is none, it station-keeps by its nearest holding until a site frees up. *Alternatives:* it replaces the nearest holding's core (that core is scrapped and the modules kept); it parks by the nearest holding and the commander picks a free site within 24 h.
- **✅ Q153 (D-165) — "A free site in a low-density sector" in a one-sector MVP (D-034).** *Recommendation:* the free site in the claimable system with the fewest holdings. Ties go to the shortest route from the fallen site, then the lowest `SiteId`. *Alternatives:* a seeded random free site; the free site farthest from the capturer's holdings.
- **✅ Q154 (D-166) — How an emergency jump travels.** Doc 15 §7 says the mother station takes "normal lane transit time" and cannot be targeted. *Recommendation:* an uninterruptible transit along the fastest route, lasting that route's time (in-system legs at cruise speed, spool-ups and lanes), untargetable and invisible like lane transit. On arrival it docks if there is capacity, otherwise it station-keeps by the core. *Alternatives:* instant relocation; normal flight that is only untargetable.
- **✅ Q155 (D-167) — The comeback grant (D-034).** "Scaled to how far behind the commander is" has no formula. *Recommendation:* the cost of one core kit for each holding the commander is behind the standings leader after the fall; at least 1 kit and at most 3; paid into the pool, ignoring the storage cap. *Alternatives:* always one core kit; 24 h of the commander's income before the fall.
- **✅ Q156 (D-168) — What a capture cancels and keeps.** *Recommendation:* queued and running builds at the holding are cancelled without refund. Its local buffers (ore, and alloy waiting for a convoy) transfer with it. The old owner's convoys to it hold or re-route by doctrine (S4). *Alternatives:* refund the old owner for cancelled builds; the buffers are lost.
- **✅ Q157 (D-169) — Disabled peripheral structures inside the window (doc 12 §3).** A solar array disabled at 18:00 is still offline when the owner's window opens at 19:00. *Recommendation:* disabled structures stay targetable, and a hit inside the window destroys one. *Alternatives:* a disabled structure must first recover before it can be destroyed; disabled structures cannot be targeted.
- **✅ Q158 (D-170) — Are protections public?** Attackers need to know when a holding becomes breachable, and so does the Strategist. *Recommendation:* each holding's `breachable_from` is public, like the window (D-075). *Alternatives:* visible only through sensors (S7); private to the owner.
- **✅ Q159 (D-171) — Catch-up minutes in the activity history.** Everyone is offline during catch-up (D-047), so a PC that is off at night moves every window, the AI's included, towards the hours the PC is on. *Recommendation:* record catch-up minutes as offline for everyone, consistent with D-047. The windows then follow the hours in which play can actually happen. *Alternatives:* skip catch-up minutes, so windows come only from live time; for AI commanders, record their schedule even during catch-up.
- **✅ Q160 (D-172) — Split S6 and order it against S5b.** Windows, presence and the order lapse need no dome. The siege lifecycle needs S5b's dome, breach event, switches and ledger. *Recommendation:* build S6a (commander records, activity and windows, presence, grace and lapse, announcements infrastructure) in parallel with S5b. S6b (defence states, capture, emergency jumps, protections, peripheral outcomes, harness sieges) follows S5b. *Alternatives:* all of S6 after S5b; S6 in one piece now, against a stub dome.

## 7. Tasks, in order

**S6a (in parallel with S5b, Q160)**
1. Commander records, presence, `SetPresence` for AI (harness), the `sieges` rules section. Snapshot format bump.
2. Activity sampling, the window, the 7-day `SetExposureWindow`, the freeze hook, `WindowMoved` announcements.
3. The disconnect grace and the lapse of direct orders (Q150).
4. The announcement plumbing in `TickOutputs`, and the read-only views for A1 to A4.

**S6b (after S5b and S4)**

5. The defence state machine: the breach handler, attackers from the ledger, the exit slot (Q146), Reinforced `immune`, the siege start check (Q148), `open`, the clock, Held (Q147 protections).
6. Capture (Q151, Q156): ownership, module transfer, caps, queue cancellation, docked-ship emergency jumps.
7. The fallen mother station: destination (Q152, Q153), the emergency-jump transit (Q154), the attacker's outpost via S4, 72 h protection, the comeback grant (Q155).
8. `structure_outcome_at_zero` with the window (Q157), the outpost exception, and the contested-gate flag if S4 lacks it. Snapshot format bump.
9. Harness `--siege` scenarios (§8) and the stakes-rule property test.
10. Docs: doc 12 notes for Q142–Q160, doc 15 §5.1's stale minimal-dome bullet (§9), doc 30's S6 row, and this plan's status.

## 8. Done when

- **CI** is green on both operating systems, with `--locked`; `cargo deny` passes.
- **Determinism:** the same seed and script give the same hash. Snapshot and restore inside Reinforced, inside a siege, inside an emergency jump and inside a disconnect grace change nothing.
- **Windows:** a commander whose online pattern shifts by 6 h moves 1 h per day and reaches the new maximum in 6 days. The window never moves while open or while frozen. The 7-day command works and is rejected on day 8.
- **Stakes rule property test:** over at least 1 000 seeded breaches with random windows, presence and fleets, every Fallen happens inside its owner's window and at least 12 h after its breach. No core is damaged outside a siege.
- **Harness `nomad-harness --siege`** runs, each from scripted commands:
  - breach → Reinforced at the computed slot → siege → Fallen and capture;
  - Held by the clock;
  - Held with no attacker online;
  - Held through a catch-up across the exit;
  - five breaches staggered as 4 + 1;
  - a mother station's emergency jump with the attacker's outpost, the 72 h protection and the grant;
  - a breach attempt during protection;
  - a peripheral structure destroyed inside the window and disabled outside it.

  This meets α's "sieges run end to end when driven by scripted commands" (doc 30 §3).
- **Validator:** every new rejection reason is covered.
- **Performance:** S6 work stays under 1% of the tick budget at stage 2 (doc 11 §4.3).

## 9. Stress test

**Conflicts found in agreed text:**
1. **D-034 against one core per site** (Q152): a mother station cannot land "on" an existing holding.
2. **D-057 against doc 15 §7** (Q151): it is unclear whether capture is open to any commander or only to the credited attackers.
3. **D-015 against D-033** (Q146): the strict "first window" reading can waste a day of a stagger slot.
4. **Doc 12 §3 against the protections** (Q147): a sector at 0 during protection has no defined outcome.
5. **Doc 15 §5.1 is stale:** it still carries the "Minimal dome … each pylon adds 25%" bullet that D-140 superseded. It needs a supersede note; no question.
6. **Doc 30's S6 row** still lists outposts and gate rules. Claiming moved to S4 (D-135). D-055 is in S3, apart from the contested flag (§4.7).

**Risks:**
1. **The 12 h notice is in-client only in the MVP** (doc 12 §5.3, doc 30 §6). A player who does not open the client learns of a breach only at the next login, at worst after the siege. The away report covers the aftermath; out-of-client notifications are hosting-era work.
2. **A switched-off PC protects everyone** (§5, Q159). AI-against-AI sieges whose start falls while the PC is off all end Held. The harness measures D-083 fast-forwarded, so acceptance is unaffected, but the 14-day human playtest may see fewer AI sieges until the AI windows have drifted to PC-on hours.
3. **"No attacker online" leaks a little private information** (D-075). It tells the defender that none of the credited attackers was online at the start. This is inherent in PR-3 and accepted.
4. **Window gaming:** a player can leave the client open at chosen hours to steer the window (Q142). The drift cap and the 14-day history bound it, and a window in hours the player is absent exposes the player.
5. **S6b sits on the critical path** after S4 and S5b (doc 30 §4). S6a in parallel (Q160) keeps the window and presence work off it.
6. **Timer bureaucracy** (doc 12 §7.3): every exit and protection is a deadline on the holding, sent as a deadline. U5 and the HUD must keep them readable.

## 10. As built in S6a

*Derived* unless marked; the *Proposed* items need the owner's approval.

- **Records and presence** (§4.1): `UniverseState` holds a record per commander, made at creation from the roster (`generate_stage1_roster`; the server's `create` names commander 1 human). Presence reads exactly the journaled `Connected` and `Disconnected` inputs, which the server sends from its sessions, at catch-up start and at a restart; `SetPresence` stands in for A5's AI schedules and is refused by the server. A `Connected` input for an AI commander, which only tests send, counts as online too. Commander deadlines (window closes, grace ends) are keyed by `(tick, CommanderId, kind)`, so they take no `SimId` and leave every other identity unchanged.
- **Grace and lapse** (§4.2): a return before the deadline cancels it; a disconnect and reconnect in one tick change nothing. A fleet already station-keeping where it is with nothing queued, the doctrine until A1, has nothing to lapse. *Proposed:* rules of engagement are a standing setting of the fleet and stay through the lapse; D-162 does not say. The owner is told which fleets lapsed (`OrdersLapsed`, for the away report). During catch-up nobody counts as online for siege rules, even inside the grace (doc 11 §7, D-047).
- **Windows** (§4.3): the record holds the occurrence open now or next as deadline ticks. At creation it is the one open at that moment, else the next; after a close it is the first that opens at or after the close, so a window moved later does not reopen at once (without this, a window moved 1 h at its close would be open again and move again within the hour). History rows are UTC days, today included. Activity drift applies from the first day; a start chosen in the first 7 days replaces that day's drift at the next close, as §4.3 reads; a choice of the current start withdraws an earlier one. The freeze (D-156) reads `defence_pending`, which S6b answers from holding states.
- ***Proposed* tie-breaks** where D-155 leaves it open: "the earliest" of equally near starts is the one earlier in the UTC day, and a target exactly 12 h away is approached by moving earlier.
- ***Proposed* defaults:** AI commanders and the harness start at 19:00 UTC until A5. `create --window` takes `HH:MM` (local), `HH:MMZ` (UTC) or `HH:MM±hh:mm`; without it the window starts at local 19:00, or at 19:00 UTC with a warning where the system cannot tell its offset.
- **Announcements** are `TickOutputs::announcements`: tick, recipients (public or named commanders) and kind; `WindowMoved` carries the next occurrence's deadlines.
- **Rejections:** `UnknownCommander` (wire `NotFound`), `InvalidWindow` and `WindowLocked`; protocol version 3, so N4, which doc 16 expects at version 3 (D-182 to D-187), takes the next one. Snapshot format 11, rules format 5.
- **For S6b:** `defence_pending` (the freeze), `counts_as_online` (D-160), `exposure_window` (the slot rule's window at the breach), `Recipients::Commanders` for the siege kinds, and the `sieges` rules section for the siege values.

## 11. As built in S6b

*Derived* unless marked; the *Proposed* items need the owner's approval.

- **Where:** `crates/nomad-sim/src/siege/` holds the defence states, protections, fallen mother stations in transit and their deadlines (snapshot format 15, after S8's 14). Deadlines fire in phase 2 after the Universe's timers and before the commanders', so a siege that ends as its window closes is over before the window may move; breaches and cores down are handled in phase 7 right after the combat that caused them. S5b's two hooks now carry the rules: `breach_may_occur` is "Nominal and not protected" (D-159), and `structure_outcome_at_zero` takes the attacker, for D-223.
- **Breach and exit** (§4.4): attackers are the commanders in the ledger with a dome hit in the 10 minutes before (`sieges.breach_credit_s`, which must fit in the ledger's window). A damaging hit on a sector already at 0 counts as a dome hit, so the commander whose hit breaches after a protection gets credit. The exit is the earliest stagger slot at least 12 h on and later than the owner's other pending exits (D-158). §5's stagger example holds for breaches after 10:00: one at 10:00 exactly gets day 1 22:00, 12 h later.
- **Held and capture:** "restores to 50%/25%" raises a sector to that share and never lowers one, since sectors regenerate while the dome is immune (D-227). A capture also clears the old owner's standing orders at the holding (rally point, REPAIR, active ability draws, STOP, turret aims, the damage ledger). *Proposed:* stored core kits stay with a captured holding, like its buffers (D-168 does not name them).
- **Capture ties** draw from the season's stream `purpose::SIEGE_TIES` (60) per holding, only when core and total damage tie (D-229).
- **Emergency jumps** are a fleet place of their own (`Place::EmergencyJump`), untargetable and taking no orders but rules of engagement (rejected as `InTransit`), so no wire reason changed and the protocol stays at version 4. Ships docked at a fallen mother station jump with it and arrive with it. *Proposed:* with no completed holding to go to, docked ships launch where they are and station-keep.
- **A fallen mother station** keeps its holding identity and its core, leaves the holdings and lands as that holding again. In transit it stays the commander's mother station for the command cap and the pool's base cap. Its builds are cancelled and its site's other modules destroyed; the capturer's outpost starts like a deployed kit (D-224). *Derived:* inside the nearest holding's system the free site with the shortest route, then the lowest `SiteId`, as D-165 orders sites. *Proposed:* "fewest holdings" counts every taken site, outposts and reserved sites included; the destination site is reserved for the jump, so a core kit cannot be deployed there (`SiteTaken`), which tells a claimant that something is on its way; a commander with no holding at all waits where the station fell; a waiting station tries again whenever an outpost is destroyed, the only way a site frees up; the landed core has 25% of its hull (`sieges.landing_core_percent`); a station's in-system legs run at 500 m/s (`sieges.station_jump_speed_m_per_s`, the WARDEN's cruise speed); the 72 h protection runs from the landing.
- **Comeback grant** (D-167): paid at the fall, the price of one kit for the commander right after the fall (with D-189's growth) times the kits, into the pool above its cap. The standings leader is S8's: `standings_now()` after the station has left its site, and `Standings::leader_holdings()`.
- **Standings (S8):** a capture, a station leaving its site and its landing call `season::mark_dirty`, so phase 9 counts the standings again on that tick (D-191). A station in its jump is out of the holdings, so `holding::counts_for_control` never sees it. S8's `scenario::place_outpost` now starts its core at 10% hull like a deployed kit (D-224).
- **Outposts:** an outpost's core at 0 destroys the outpost at any time (D-054). S5b counted an outpost's core as inside a dome, so raiders could not damage it; it now counts as inside only where a dome exists (D-056, D-163). One raider strike craft deals exactly an outpost core's D-224 growth, 37.5 hull a second, so it alone never destroys one.
- **Peripheral structures** (D-138, D-169, D-223): destroyed at 0 inside the owner's window, disabled outside it; raiders only disable. Protection does not change this: it only prevents a breach (D-034).
- **D-228:** the solar slot is at grid (−62, 0) and the deposit slots at (50, 0) and (0, 50), on the north-west, south-east and south-west sector boundaries, wholly outside the dome and clear of the line of fire from every sector's centre; the rules check rejects a peripheral circle that reaches the dome. `--combat breach` breaches from the north in the same 452 s as from the east.
- **Tests:** unit tests for every step of the lifecycle, snapshots inside Reinforced, a siege and a jump, and the stakes-rule property test over 1,002 seeded breaches (ignored in CI, D-190). `nomad-harness --siege` runs §8's eight scenarios end to end.
- **Announcements** gain `Breached`, `SiegeStarted`, `Held` (owner and attackers), `Captured`, `MotherStationFell`, `StationLanded`, `OutpostDestroyed` (public) and `MotherStationJumped`, `FleetsJumped` (owner). Rules format 9, after S8's 8.
