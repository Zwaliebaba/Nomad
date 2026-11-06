# 01 — Game concept

Status: **Draft; round-2 decisions applied.** Sections 2–3 are kept as the original analysis. Section 4 shows each proposal's status, and section 5 reflects the agreed MVP. This document separates what the owner has *stated*, what those statements *force*, where the concept is weakest, and what I *propose*. Nothing proposed here counts as decided until it is recorded in `03-decision-log.md`.

## 1. Stated requirements

| ID | Requirement | Source |
|---|---|---|
| R-01 | A multiplayer space RTS, implemented in Rust. | Owner |
| R-02 | The **Universe** is persistent and keeps evolving while players are disconnected. | Owner |
| R-03 | When a player is not connected, a **Deputy** takes over. It keeps the standard actions running and defends the player's assets. | Owner; refined by D-013 (the Deputy is always on) |
| R-04 | The Universe is sized for **up to 100 players**, as a target for now. | Owner; one Universe of up to 100 commanders (D-021) |
| R-05 | **MVP:** limited scope, playable by **one human player with up to 9 AI players**. | Owner |
| R-06 | Terminology: the total game space is called the **Universe**. *World* stays fine as a technical term. | Owner |
| R-07 | Look and interaction follow the phase-1 package: an isometric voxel diorama with a fixed-pitch camera, 4-way snap rotation and a military HUD. The premise is that the player starts with a mother station and grows an empire of factories and shipyards. | Owner, `phase1-visual-slice/` |

## 2. What the requirements force (derived)

These follow from R-01 to R-07 and leave no real design choice. Only the way each one is implemented is still open.

**C-01: A simulation host that runs independently of any client.** R-02 means the Universe advances while nobody is connected, so the authoritative simulation cannot live inside a game client. This holds for the MVP too, unless the MVP is allowed to pause the Universe when the human quits (Q11).

**C-02: A server-authoritative model; deterministic lockstep is ruled out.** Classic RTS lockstep needs every participant to simulate every tick. That is impossible when commanders are offline, players join and leave at will, and 100 empires run continuously. Clients send commands, the server simulates, and clients receive state they are entitled to see. A deterministic simulation is still worth having for replays, server-side testing and desync-free reconnects, but it is not a networking requirement.

**C-03: The client is a view, never an owner of state.** The phase-1 rule that "the renderer reads only from the fixture" is exactly the right seam. `siege_kr114.json` is effectively the first draft of the **client view-model** that the server will later feed.

**C-04: The Deputy exists from the first multiplayer build.** The first time a human disconnects, something has to act for them. If the MVP keeps the Universe running (Q11), the Deputy is MVP scope.

**C-05: Simulation cost scales with the size of the Universe, not with the number of connected players.** All 100 empires are simulated at all times, whether one player or sixty are online.

**C-06: Durable Universe state.** A persistent Universe has to survive server restarts, crashes and deployments without losing more than a bounded window of play.

**C-07: Every rate and timer must be designed against a 24-hour human cycle.** See W-08 below. Income per minute, build times and travel ETAs all run while the player sleeps.

## 3. Stress test: where the concept is weakest

This is ordered by how much damage each point does if it is left unsolved.

### W-01: The offline asymmetry is the core design problem, not a side feature

A typical player of a persistent strategy game is online 1–3 hours a day, so their empire is under Deputy control **roughly 85–95% of the time**. That has three consequences:

- Most of the game's combat will be fought with at least one side run by a Deputy.
- Rational attackers will strike when the target is offline. In PvP, "attack sleeping players" becomes the dominant strategy unless the rules make it unprofitable.
- "I logged in and my station was gone" is the best-known churn driver in this genre.

The phase-1 scenario makes this concrete. "Hold MS-01 for relief, VANTH ETA 04:00" is a siege decided in **minutes**. If the defender is asleep, that siege is decided entirely by the Deputy. So R-03 ("defends the objects") is not enough on its own. The design also needs a rule for **what an attacker can achieve against a Deputy-defended holding, and how fast** (Q7).

Known families of mitigation, not yet a choice:

- **Reinforcement timers.** A Deputy-defended structure drops into a hardened state, and the decisive phase resolves later at a time the defender can attend.
- **Defender-chosen vulnerability windows.**
- **Caps on what can be lost per offline period.**
- **Post-attack protection.**
- **Allied cover**, where online allies can relieve or take over defence. The phase-1 HUD already hints at this with relief fleets and the commanders list.

The phase-1 sectored shield is a natural mechanical hook for reinforcement.

### W-02: The Deputy competence paradox

- If the Deputy is weak, offline players get farmed (W-01).
- If it is strong, being online stops mattering. Worse, a good micro-AI out-plays most humans in tactical control, so players would hand control to the Deputy *while online*.

The resolution has to come from the Deputy's **authority**, not its **skill**. Let it be competent at a bounded job, and reserve initiative for the human: offence, expansion, diplomacy, spending strategy. This matches the owner's wording ("standard actions … defended"), but "standard actions" needs a precise list (Q6).

### W-03: The MVP may not test the risky part

"One human and 9 AI players" can be built in two ways:

- As a single-player game where the Universe pauses on exit. That is cheaper, but it never exercises R-02 or R-03. Persistence, the Deputy and server authority would then be retrofitted later, and retrofitting multiplayer onto a single-player architecture is the classic way these projects fail.
- As a headless server that keeps running plus a client, even on one PC. That costs a little more up front and tests the actual product thesis.

### W-04: Snowballing in a Universe that never ends

In a forever-persistent Universe:

- Early and high-hours players compound their advantage.
- New players joining a mature Universe have no viable entry point.
- Balance changes hit live empires that were built under the old rules.

R-04's "up to 100 players" also leaves open whether those 100 can ever turn over. The usual answers are seasons with a win condition and a reset, or strong catch-up and new-player protection, or both (Q2).

### W-05: Observed and unobserved battles must use the same rules

To save CPU, it is tempting to simulate battles that no human is watching at lower fidelity. If the coarse model produces different outcomes from the full one, players will exploit it by logging in, or staying out, to change results. **Any fidelity reduction must be outcome-equivalent.** Otherwise, simulate everything at full fidelity. A rough sizing (below) suggests that is affordable at this scale.

Rough sizing, to be validated by a prototype:

- 100 commanders × ~30–60 ships (the COMMAND cap is 200, and an interceptor costs 6) gives about 3–6k ships.
- About 20 modules per holding × a few holdings each gives a few thousand structures.
- Projectiles peak in the low thousands during large battles.

That is on the order of 10⁴ simulated entities. At a 10 Hz authoritative tick with spatial partitioning, this fits **one server process** in Rust with headroom. The 100-player target therefore does not justify a distributed or sharded simulation. It does justify partitioning the simulation **by system** internally, so it could be split later.

### W-06: The AI commanders and the Deputy are the same problem at different scopes

A Deputy needs operational and tactical competence: run queues, route power, launch defenders, repair, retreat. An AI commander needs exactly that **plus** strategy: expansion, offence, diplomacy, long-term spending. Building them separately doubles the work and lets the two drift apart in quality. Building the AI commander as Deputy plus a strategic layer means every MVP game session also tests the Deputy.

### W-07: Alliances are implied but not specified

The phase-1 HUD shows allied commanders, relief fleets and "4 / 6 COMMANDERS". The design needs answers to several questions here:

- Are alliances formal?
- Can allies see each other's holdings, or defend them?
- Can an ally's Deputy coordinate with an online human?
- In the MVP, are the 9 AI commanders potential allies, fixed enemies or free agents? (Q15)

### W-08: Economy rates do not survive a 24-hour cycle

The phase-1 fixture shows ALLOY 21,860 at +312/m. A player offline for 8 hours accrues 312 × 480 ≈ **150k alloy**, about seven times their current stock. Either that is intended and every cost scales to match, or the economy needs storage caps, decay or upkeep. Each of those changes the player's login rhythm, which is itself a design goal to set deliberately ("check in twice a day", or "play a 2-hour session every few days").

## 4. Proposals for the owner to accept or reject

| ID | Proposal | Addresses | Status |
|---|---|---|---|
| P-01 | **The Deputy executes doctrine; it does not strategise.** It runs owner-authored standing orders (build queues, convoy routes, patrols, power priorities, rules of engagement) plus a fixed defensive repertoire. It never starts offence, expansion or diplomacy. The same mechanism is available while the player is online, as delegation per holding or per fleet, so the Deputy is a feature that gets used and tested every session, not a fallback that is only discovered when it fails. | W-01, W-02 | Accepted, refined (D-013, D-016) |
| P-02 | **One AI stack.** AI commander = Deputy (operational and tactical) + Strategist (strategic). The MVP's 9 AI players use it. | W-06 | Open: to be detailed in doc 13 |
| P-03 | **The MVP is client–server from day one.** It has a headless authoritative server, localhost is fine, and the Universe keeps running when the client closes. | W-03, C-01 | Accepted (D-026) |
| P-04 | **One authoritative simulation process for ≤100 players,** partitioned internally by system, with snapshots plus a command journal for durability. No sharding until measurements demand it. | W-05, C-05, C-06 | Consistent with D-021 and D-030; to be decided in doc 20 |
| P-05 | ~~**Offline protection through the shield.** When a Deputy-defended holding comes under siege, the shield enters a reinforced state, and the decisive phase is deferred to a window the defender can attend.~~ **Withdrawn (round 2):** tying protection to Deputy control is exploitable, because a defender could log off to become protected. Replaced by the clock-based stakes rule in `12-offline-play-and-deputy.md`. P-01 is refined there as well (PR-3, PR-4). | W-01 | Withdrawn |
| P-06 | **Seasonal Universes** with a win condition. Balance changes land between seasons. | W-04 | Accepted (D-020) |

## 5. MVP framing

**Stated:** one human, up to 9 AI players, limited scope (R-05).

**Agreed (round 2):**

- **Architecture:** a headless authoritative server keeps the Universe running when the client closes; localhost is fine (D-026). The server runs locally next to the Windows client and is built to be containerised for later hosting (D-029). Stack: `hecs`, `tokio` + `quinn`, SQLite (D-030).
- **Loops:**
  - economy and building, with storage caps (D-025);
  - fleets and combat, including the shield and siege lifecycle (D-015);
  - travel between systems through gates and lanes (D-022).
  - Diplomacy is out (D-026).
- **Staging:** the first MVP acceptance is 3 AI commanders plus NPC raiders in one sector. The AI stack is built for up to 9. Vacation and derelict states are left out (D-038).
- **Opponents:** up to 9 fair AI commanders in a free-for-all, with simulated online and offline schedules, so the human meets Deputy-run empires and their own empire runs under its Deputy (D-027). NPC raiders are part of the Universe (D-014).
- **Winning:** a season is won by system control (D-020). Commanders are never eliminated (D-017).
- **Sequencing:** phase 1 first, with a headless `nomad-sim` track built in parallel (D-028, D-037).
- **Information:** sensor-based fog of war (D-039).

**Derived:**

- An MVP Universe big enough for 10 commanders plus NPC raiders, with several systems per commander so that capture and system control mean something. Sizing goes in doc 10.
- There is no time compression (D-036). Human playtests of the siege cycle take at least 13 h. Fast iteration on pacing therefore happens in the headless simulation (D-037), which runs AI-against-AI faster than real time.

**Relationship to phase 1:** phase 1 is a client-only visual slice, and its fixture-driven HUD is the contract the simulation will later feed. The fixture schema should grow into the server-to-client view-model, not be replaced by one.
