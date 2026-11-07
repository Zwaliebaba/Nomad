# Work package A3 — The raider director

Status: **Planned and decided** (Q220–Q226 → D-219 to D-225). Written together with `A1-deputy.md`, whose §4 holds the shared architecture, and `A2-strategist-economy.md`. Builds on A1a (the `nomad-ai` crate, outbox, competence gate), S5a (built), S5b (module damage; built), S6b (the outpost exception, peripheral outcomes by window) and S7 (NPC observers). A3 completes MVP-α, so §5 also defines the α acceptance runs shared by S8, A1, A2 and A3. This plan is also the brief for whoever builds A3. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. Goal

NPC raiders put every commander under pressure on a seeded rhythm (D-081):

- each haven launches a raid every 2–4 h;
- the target is weighted by asset value and distance;
- the group is sized at 30–50% of the target's fleet value in that system;
- it raids convoys, ships outside docks, outposts under construction and peripheral structures;
- it never damages a dome (D-056);
- it withdraws after 10 min or after losing half its strength;
- it never raids the same commander twice within 6 h;
- no raid starts during catch-up (D-047).

The raider director sees only its faction's view (D-077, D-181) and commands its groups with ordinary fleet commands through the single validator. With A1 and A2 this gives α its "raids happen" criterion (doc 30 §3).

## 2. Inputs

- **Raiders:**
  - D-081 and doc 13 §8: timing, targets, strength, withdrawal, the 6 h rule, none during catch-up;
  - D-014: PvP plus NPC raiders;
  - D-056: zero damage to domes, guaranteed mechanically;
  - doc 15 §10: "raider havens send out raid groups: strike craft, and sometimes a heavy cruiser"; peripheral structures "can only be disabled, as for anyone";
  - D-163: raiders deal no damage to modules inside the dome.
- **Targets and their rules:**
  - D-054: outposts under construction can be destroyed at any time;
  - doc 12 §3 and D-169: peripheral structures are destroyed inside the owner's window and disabled outside it;
  - D-149: recovery of disabled structures;
  - D-138: harvesters and extractors follow the peripheral rule.
- **Information:**
  - D-077 and doc 16 §7: raiders see through their faction's sensors, under the same rules;
  - D-181: NPC factions have visibility sets and lane traffic but no last-known memory, and the director works from public data and live sensing;
  - D-075: online status is private and plays no part;
  - D-074 and D-179: departures are observed.
- **Topology and travel:**
  - doc 10 §4.4 and D-099 item 6: one haven, no sites, one gate to the hub;
  - D-055: arrival scatter, the 10 s shroud, the 5 s spool-up;
  - D-042: lane transit cannot be interrupted.
- **Units:** doc 15 §12 (raider strike craft and HSTL stats), D-101, D-188.
- **Catch-up:** D-047, doc 11 §7.
- **Code:**
  - `crates/nomad-sim`: `Owner::Npc(FactionId)`, `ShipClass::RaiderStrikeCraft` and `HeavyCruiser`, the rules' raider torpedo with 0 against domes, the stage-1 generator's haven, `catching_up()`, `validate_command(state, issuer: Owner, …)`;
  - no NPC fleets are spawned yet.

## 3. Scope

**In scope:**

- **The launch rule:** timer, eligibility, target draw, strength and composition, spawning and removing NPC fleets.
- **The director:** travel, arrival, target selection, engagement, early withdrawal, the 10-minute clock, the loss limit, the return.
- **A `raids` part** of the rules file's `ai` section (A1 Q212).
- **The rules hooks raiders need:** what raiders do to peripheral structures (Q224), and an outpost's hull while it is built (Q225).
- **Harness:** `--raids`, and **the α acceptance runs** (§5).

**Out of scope:**

- A raider economy, bases or growth (Q222: groups are spawned per raid).
- Raider sieges (never, D-056) and haven defences.
- Raider diplomacy, and difficulty for raiders beyond D-081.
- The second haven at stage 2 (the same code with two havens).
- Notifications and the away report's presentation (U5).

## 4. Design

### 4.1 Rules and AI: who decides what

*Proposed* (Q220).

- **The launch is a simulation rule:** when (a timer), whether (eligibility), whom (a weighted draw from public data), how strong and with what (from the target's real fleet value), and the spawn.
  - It is a game rule like a kit's price (D-189), not an AI decision.
  - It reads hidden state only to size the group, and tells nobody what it read.
- **The raid group's behaviour is the director's:** an AI layer in `nomad-ai` reading the faction's `View` (its visibility sets, lane traffic and public data, D-181). It issues ordinary fleet commands as `Owner::Npc` through the outbox and the validator (D-077, A1 §4.3).
- **Why:** D-081 sizes raids by "the target commander's fleet value in that system", which the faction cannot see. Between raids it has no ships and so no sensors at all (its haven has no sites, D-099). Putting the sizing in the rules keeps D-081 and D-077 both true.

### 4.2 Launch timing

*Agreed* (D-081, D-047); details *Proposed*.

- **Each haven has a launch timer,** fired in phase 2. The first fires 2–4 h after tick 0. Each firing draws the next interval uniformly from [2 h, 4 h] on the haven's `RAIDS` stream (A1 §4.6).
- **During catch-up** a firing launches nothing and draws its next time as usual (D-047: no new raids start). A raid is not deferred to the end of catch-up, which would raid a player the moment their PC starts.
- Groups already in the Universe keep raiding during catch-up. Only new raids are barred (*Derived* from D-047's wording).

### 4.3 Target choice

*Agreed* (D-081): weighted by asset value and by distance from the haven; no commander twice within 6 h; online status plays no part. The weights are *Proposed*.

- **Candidates:** each (commander, system) where the commander has a holding or an outpost in a claimable system reachable from the haven, and the commander was not the target of a launch in the last 6 h.
- **Weight,** from public data only (ownership D-044, deposits D-066, topology):
  - (deposit value of its holdings and outposts there + 1 per holding + 2 per outpost under construction) ÷ (1 + jumps from the haven);
  - rules values.
- **The draw:** weighted, on the haven's stream. With no candidate, no raid; the timer draws again.
- *Derived:* at stage 1 the hub is 1 jump from the haven, the standard systems 2 and the home systems 3 (doc 10 §4.4). Raids therefore fall most on the contested centre, and on a commander's home only while it has nothing nearer the haven.

### 4.4 Strength and composition

*Agreed* (D-081: 30–50% of the target's fleet value in that system; doc 15 §10: strike craft, sometimes a heavy cruiser); the value and the floor are *Proposed* (Q221).

- **Fleet value:** the sum, over the target commander's ships in that system (docked or in space), of their build cost as alloy + 3 × crystal (D-066's ratio):
  - VX 1,200, LANCER 2,300, WARDEN 5,800, MULE 1,500.
- **NPC values** are rules values: raider strike craft 1,600 (about 70% of a LANCER by hull and torpedo) and HSTL 17,000 (about three WARDENs by hull and batteries). Both are starting values, tuned at α.
- **Budget:** a draw from [30%, 50%] on the haven's stream × the fleet value.
- **Composition:**
  - one HSTL if the budget covers it plus two strike craft;
  - strike craft for the rest, rounded down;
  - at least two strike craft, so an undefended target still sees a raid.
- *Example:* the start kit's 3 VX and 2 MULE are worth 6,600, a budget of 1,980–3,300, so a raid on a fresh home system is 2 strike craft.

### 4.5 Spawn, travel and lifecycle

*Proposed* (Q222).

- **Spawn:** a new fleet owned by `Owner::Npc(faction)` appears in the haven's gate approach (D-055's scatter), at full hull, on weapons free. From then on it is a fleet like any other: S3 moves it, S5 fights it, S7 senses it.
- **Travel:** the director orders `Jump` to the target system (S3 routes). Transit is uninterruptible and invisible (D-042, D-074).
  - Commanders sensing a gate the group departs from see where it is going and when it arrives (D-179). The haven's own gate is covered by nobody.
  - At stage 1 the trip takes 40–70 min to the hub, 80–140 min to a standard system and 120–210 min to a home system.
- **End:** after the raid, the group jumps back to the haven and is removed on arrival. Raiders have no economy and do not accumulate.

### 4.6 The director's control

*Proposed;* the director reads only its faction's view (D-077, D-181).

- **When:** at 2 Hz per group (phase 4, A1 §4.5), at Normal's competence: 0.6 s latency, 2 commands per second per group (A1 §4.3).
- **On arrival,** when the shroud ends (D-055), the 10-minute clock starts (Q223).
- **Targets,** in priority order among those it senses in the system:
  1. MULEs: convoys and kit carriers;
  2. outposts under construction;
  3. ships in space worth less than the group;
  4. peripheral structures, solar arrays first (they cut regeneration and production).

  Within each, targets outside sensed turret reach come first: 2.5 km from turrets, whose holdings reveal their modules (D-175).
- **With nothing sensed,** `AttackMove` to the target holding's site, whose position is public, sensing on the way.
- **Stronger forces:** if the sensed hostile value within 5 km exceeds twice the group's remaining value, it withdraws early.
- **Never** a dome or a module inside one. They would take no damage (D-056, D-163), so the director does not waste the clock on them.

### 4.7 Withdrawal

*Agreed* (D-081: after 10 min, or after losing 50% of the group); the details are *Proposed* (Q223).

- **The clock** runs from arrival in the target system.
- **Losses** are counted by value against the spawned value.
- **Withdrawal** is a `Jump` back to the haven, where the group is removed.

### 4.8 What raiders may damage

*Agreed* where marked; Q224 and Q225 are *Proposed*.

| Target | Raiders | Source |
|---|---|---|
| Domes | No damage, mechanically | D-056 (S5a, built) |
| Modules inside the dome | No damage | D-163 |
| Ships outside docks, convoys | Destroyed as for anyone | D-081 |
| Outposts under construction | Destroyable at any time; the hull while building is Q225 | D-054, S6b |
| Peripheral structures (solar, harvester, extractor) | Disabled, never destroyed, even inside the owner's window (Q224) | D-081, D-138 |

### 4.9 Events

*Derived.*

- Launches and raid endings are harness outputs, not announcements. Nobody is told a raid is coming except by what their sensors show (D-074).
- The target learns what happened from its own events: its losses, its disabled structures and its sightings (A1 §4.4). Its away report is built from those (U5).

## 5. The α acceptance runs

Shared by S8, A1, A2 and A3. The criteria are *Agreed* (doc 30 §3, D-083); the runs and measures are *Proposed* (A2 Q219, Q226).

**Harness modes:** `nomad-harness --season [--seeds N] [--weeks 8] [--solo] [--online] [--scripted-sieges]`. They run 4 AI commanders with A1, A2 and A3 at Normal on their seeded schedules (A1 Q205) against the stage-1 sector and its haven, in a release build.

- `--solo`: one AI and the raiders, the other commanders idle, for "unopposed".
- `--online`: every AI always online, as a bound.
- `--scripted-sieges`: see criterion 5 below.

| α criterion (doc 30 §3) | Measure | Source |
|---|---|---|
| 1. AI commanders, raiders and Deputies play seasons headlessly | 3 seeds × 8 weeks run to an S8 result without a panic; standings history reported | S8, A1–A3 |
| 2. Determinism holds (D-049, D-083 item 1) | Each seed run twice ends on the same hash; snapshot and restore at 3 seeded ticks ends on the same hash | All |
| 3. Economy pacing within ±50% | D-070's six measures, medians within ±50% | A2 Q219 |
| 4. Raids happen | Q226's measures | A3 |
| 5. Sieges run end to end by scripted commands | S6b's `--siege` scenarios; and `--scripted-sieges`: the harness gives commander 1's fleets a scripted breach and siege against an AI holding in its owner's window, while that holding's Deputy defends | S6b, A1 |
| D-083 item 2: no idle income | ≥ 90% of income spent over every 24 h, or at the pool cap with nothing to build | A2 |
| D-083 item 3: expansion | `--solo`: 4 or more holdings by day 3 | A2 |
| D-083 item 6: nothing stuck | A monitor flags any fleet whose order or plan step makes no progress for 10 min | A1, A2, A3 |
| D-083 item 7: budget | Strategist mean ≤ 5 ms per evaluation; Deputies and the director ≤ 25% of a 500× tick | A1 Q204 |
| D-083 item 8: fairness | `nomad-ai`'s manifest test; the per-layer command counters reconcile | A1 §4.8 |

- **Not measured at α:** D-083 item 4 (battles won with the counter triangle) and item 5 (sieges happening in AI play). Both need A4.
- **Also reported:** fast-forward speed per run, catch-up speed with Deputies, AI command rejections by reason, and S5's TTK scenarios with S7 detection and the Deputy's default ability use (S5 plan §9 item 5, D-147).
- **Tuning items found before α** (owner, 2026-11-07: left to the α runs): one raider strike craft deals exactly an outpost core's D-224 growth, 37.5 hull a second, so a lone raider never destroys an outpost under construction (S6 plan §11); the runs report outpost losses to raids by group size. S4's 7-day harness also put storage fill (19.6 h) and fleet rebuild (175 h) outside D-070's targets, and the counter triangle failed at its starting values (D-137). *Derived:* those three were tuned before α (D-279): D-281 counts the diminishing yield per resource, D-282 sets F, storage and the crystal rate, and D-284 raises LANCER hull to 1,400. Build-out is measured directly and fails (D-283); it is an α item for A2's build priorities (D-280). *Proposed:* the α runs recheck raid strength against LANCER escorts, since a lone raider strike craft now needs 56 torpedo hits to kill one (D-284).
- **Cost:** at the doc 11 target of 500× in a release build, an 8-week season takes under 3 h per seed. The runs are ignored tests, run locally (D-190).

## 6. Timeline check

*Derived* from the agreed values; behaviour as proposed here.

**A raid on an escorted outpost in the hub.** Day 2. C2 deployed an outpost on a rich hub site at 08:30 (complete at 10:30), escorted by 2 VX, with its MULE docked elsewhere.

| Universe time | Event | Rule |
|---|---|---|
| 09:00 | The haven's timer fires. C2 in the hub weighs most: an outpost, rich deposits, 1 jump. C2 was last raided more than 6 h ago | §4.3, D-081 |
| 09:00 | C2's ships in the hub: 2 VX, worth 2,400. Budget 30–50%: 720–1,200, below the floor, so 2 strike craft spawn at the haven's gate | Q221 |
| 09:00–09:55 | Spool-up, then 55 min in lane transit, invisible. Nobody covers the haven's gate | D-055, D-074 |
| 09:55 | Arrival at the hub gate. The outpost carries no sensor (D-176), and the VX see Light targets at 15 km × 0.6 = 9 km, so C2 sees the raiders only once they are within 9 km of the escort | D-176, D-072 |
| 09:56 | Shroud ends; the 10-minute clock starts | Q223 |
| 10:04 | Raiders reach the outpost. Under Q225 its hull is about 80% of 300,000; two strike craft do about 75 per second to it, so the outpost is out of reach. They take the VX instead | Q225 |
| 10:05 | The VX (100% against Light) win; the raiders reach 50% losses and withdraw | D-052, D-081 |
| 10:30 | The outpost completes | D-054 |

The escort decided it. Without one, the raiders would have had nothing to kill in time and would still have withdrawn. Travel time and construction time are comparable, so raids catch outposts only near the haven (§10 item 1).

## 7. Questions

Numbered Q220–Q226; A1 has Q200–Q212 and A2 Q213–Q219 (Q227–Q229 unused). Each has a recommendation and alternatives.

- **✅ Q220 (D-219) — Which parts of the raider director are rules and which are AI.** D-081 sizes a raid by the target's fleet value in that system, which the faction cannot see (D-077, D-181).
  *Recommendation:* the launch is a simulation rule: a timer, eligibility, a target drawn by weights from public data, strength and composition from the target's real fleet value, and the spawn. The director AI only controls raid groups, through the NPC view and ordinary commands. *Alternatives:* everything is AI through the view, with strength from public data only (for example the target's holdings in that system), which drops D-081's fleet-value scaling; strength from the fleet value the faction sensed at its previous raid on that commander, with the floor for a first raid; a declared exception to D-077 letting the director read fleet values.
- **✅ Q221 (D-220) — Fleet value, and a floor.** D-081 does not define "fleet value", NPC classes have no cost (doc 14 §9.4), and 30–50% of nothing is no raid.
  *Recommendation:* build cost as alloy + 3 × crystal (D-066's ratio), docked ships included; NPC classes valued as rules values (raider strike craft 1,600, HSTL 17,000 to start); a floor of 2 strike craft, so undefended convoys and outposts are raided too. *Alternatives:* command points, with command values for the NPC classes; alloy cost only; no floor, so a target with no ships in the system is never raided.
- **✅ Q222 (D-221) — Where raid groups come from, and where they go.**
  *Recommendation:* each launch spawns a fresh group at the haven's gate, and the group is removed when it returns. There is no raider economy, and survivors do not accumulate. *Alternatives:* a persistent haven garrison that regrows at a fixed rate and sends groups from it (raids weaken after losses); survivors return to a pool that the next raid draws from first.
- **✅ Q223 (D-222) — When the 10 minutes start.**
  *Recommendation:* at arrival in the target system, when the shroud ends. Losses count by value against the spawned group. Withdrawal is a jump back to the haven, earlier if the sensed hostile value within 5 km exceeds twice the group's. *Alternatives:* from the first engagement, with a 30-minute cap in the system (more raid time against hard-to-find targets); from the launch (distant raids would expire in transit).
- **✅ Q224 (D-223) — Raiders and peripheral structures inside the owner's window.** D-081 and doc 13 §8 say peripheral structures "are disabled only". Doc 15 §10 says "as for anyone", and D-169 destroys them inside the window.
  *Recommendation:* raiders only disable peripheral structures, at any time. That matches D-081's wording and its intent (raids pressure; they do not wipe out). S6b's `structure_outcome_at_zero` takes the attacker's owner into account. *Alternatives:* the same rule as for commanders (destroyed inside the owner's window, D-169); raiders destroy them inside the window only when the owner has no ship in that system.
- **✅ Q225 (D-224) — An outpost's hull while it is built.** S4 gives the core its full 300,000 hull from the start (S4 plan §4.7). Two strike craft deal about 75 per second to a structure, so a raid cannot destroy an outpost in 10 minutes, and D-054 and D-081's outpost target exist on paper only for raiders.
  *Recommendation:* a core under construction has a hull that grows linearly from 10% to 100% of its maximum over the 2 h deployment. A fresh claim is fragile and a nearly finished one is not, which rewards escorting it early. *Alternatives:* a fixed low hull (for example 30,000) until complete, then full; keep the full hull from the start (only fleets of strike craft can contest claims).
- **✅ Q226 (D-225) — What "raids happen" means at α.**
  *Recommendation:* in every α season run:
  1. each haven launches every 2–4 h, with every interval inside the bounds;
  2. every commander is the target of at least one raid per 24 h on average, and never twice within 6 h;
  3. at least half of the raids that reach their system destroy or disable something;
  4. no dome ever takes raider damage;
  5. no raid launches during catch-up (one run includes a catch-up segment).
  
  *Alternatives:* only "at least one raid per commander per day"; the recommendation plus a cap, on average, of 30% of the target's fleet value in that system lost per raid ("pressure, not wipe-out"); the recommendation plus a minimum share of raids that destroy a MULE or an outpost.

## 8. Tasks, in order

1. **Rules:** the `raids` part of the `ai` section: interval, 6 h spacing, weights, NPC values, floor, clock, loss limit, early-withdrawal ratio. An NPC faction record per haven. Rules format bump.
2. **The launch rule** (Q220–Q222): timer, eligibility, target draw, strength and composition, spawning at the haven's gate and removal on return, the catch-up skip. Snapshot format bump; pins re-recorded.
3. **The director** (`nomad-ai`): travel, arrival, target priority, engagement, early withdrawal, the clock and loss limit (Q223), the return, through the NPC view and the outbox.
4. **Rules hooks:** the attacker-aware peripheral outcome (Q224) in S6b's hook; the outpost hull while building (Q225), as an S4 and S6b amendment.
5. **Harness `--raids`:** raids per commander and per day, outcomes, the 6 h spacing, catch-up suppression, dome damage, raid losses.
6. **The α runs** (§5): the `--season` modes and the acceptance report.
7. **Docs:** doc 13 §8 notes for Q220–Q226; doc 15 §10 for Q224 and Q225; doc 30's A3 row and α status; this plan's status.

## 9. Done when

- **CI** is green on both operating systems, with `--locked`; `cargo deny` passes.
- **Determinism:** the same seed gives the same raids and the same hash. A snapshot taken during a raid group's transit, and one taken during its 10 minutes, restore identically.
- **Rule tests:**
  - launch intervals stay within 2–4 h;
  - no commander is targeted twice within 6 h;
  - no launch during catch-up, while groups already out continue;
  - strength is 30–50% of the target's fleet value in that system, with the floor;
  - weights use public data only (a test changes hidden state and finds the same target draw).
- **Director tests:**
  - priorities as in §4.6;
  - no command against a dome or a module inside one;
  - withdrawal at 10 min and at 50% losses;
  - early withdrawal against a stronger force;
  - removal at the haven;
  - every raider command passes the validator as `Owner::Npc`.
- **Q224 and Q225:** a raider never destroys a peripheral structure; an outpost's hull follows its construction progress.
- **α** (§5): every criterion in the table passes, or is listed with its cause and owner for A6.

## 10. Stress test

1. **Raids arrive late for outposts.** A raid needs 40–210 min to arrive and an outpost completes in 2 h, so raids catch outposts only in the hub and its neighbours. That is the geometry of doc 10 §4.4 and D-043's lane times, not a bug, but it weakens D-081's outpost target. Q225 makes fresh outposts vulnerable; the arrival delay remains.
2. **Raids may find little to hit.** With depots in every system (A2 Q217) convoys are rare, garrisons stay docked, and the main effect is disabled solar arrays. The α measures (Q226 item 3) show whether raids still bite; the levers are numbers (the storage price, raid values).
3. **Sizing from hidden values** is a rule, not perception (Q220). The target's defence can change during the trip, so a raid sized to 2 VX can meet 6. That is fair: the defender chose to reinforce.
4. **Distance weighting makes homes quiet.** Once commanders hold sites in the hub and standard systems, raids rarely reach home systems. Raids then mostly pressure expansion, which is the intent (doc 10 §4.4: "raiders reach the whole sector through the hub").
5. **Predictability.** Fixed weights let players learn that hub outposts draw raids. Seeded draws keep timing and targets uncertain; A6 can add noise.
6. **Spawned groups never deplete,** so killing raiders achieves only the raid's failure. That keeps raiders a constant pressure, as D-081 wants, and avoids a raider economy.

## 11. Conflicts and gaps found

1. **D-081 against D-077 and doc 16 §7:** raid strength and target weighting use values the faction cannot sense (Q220).
2. **D-081 and doc 13 §8 against doc 15 §10, doc 12 §3 and D-169:** "disabled only" against "as for anyone" and destruction inside the window (Q224).
3. **D-054 and D-081 against S4's full hull for an outpost under construction** (S4 plan §4.7, as built): raiders cannot destroy an outpost in their 10 minutes (Q225).
4. **D-081's "fleet value" is undefined,** NPC classes have no cost, and 30–50% of zero gives no raid (Q221).
