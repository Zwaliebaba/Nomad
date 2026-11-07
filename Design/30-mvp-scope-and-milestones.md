# 30 — MVP scope, milestones and acceptance

Status: **Agreed (round 10).** Q88–Q91 were decided as D-094 to D-097. D-135, D-139, D-172 and D-205 later amended the packages and the order of work (§2, §4); D-190 moved the integration test out of CI (§3, §5). D-235 to D-237 (doc 21) make the browser the player's client, run the server in Docker Desktop, and add the MP-1 milestone (§2, §3, §6); the packages they bring are *Proposed* until Q230–Q243 are answered. Built on R-05, D-026, D-028, D-037, D-038, D-051, D-058, D-070 and D-083, and docs 10–17 and 20. Labels: *Agreed* means a decision ID is given, *Derived* means it follows from agreed decisions, and *Proposed* means it needs the owner's approval.

## 1. What the MVP is

*Derived* from agreed decisions.

**The game:** a persistent, single-player Universe. One human plays from a browser on Windows (D-235) against **3 fair AI commanders and NPC raiders**, in one sector of 9 systems (D-038, doc 10 §4.4).

**The setup:** a local headless server that keeps running when the client closes and catches up after the PC was off (D-026, D-047). It runs in Docker Desktop on the player's PC, for one player, without internet and without a login (D-236, doc 21).

**What it includes:**

- economy and building (doc 14);
- fleets and combat (doc 15);
- the offline protection rules with sieges and capture (doc 12);
- fog of war (doc 16);
- the Deputy and the AI (doc 13);
- strategic views and UI (doc 17);
- seasons that end with a winner by system control (D-020).

**Acceptance happens at stage 1** (D-038). **Stage 2**, with up to 9 AI commanders in two sectors, follows as its own milestone (R-05).

## 2. Work packages

Sizes are relative: S < M < L < XL. Status as of this document: ✅ done, ⏳ in progress.

| Track | Package | Content | Size |
|---|---|---|---|
| **Simulation rules** | S1 ✅ | Simulation core (doc 11) | — |
| | S2 ✅ | Universe structure, deposits, generator invariants (doc 10, D-066, D-098, D-099) | M |
| | S3 ✅ | Movement, fleets, orders, cruise mode, jump transit (doc 15 §3, D-042; plan in `plans/S3-movement-and-orders.md`) | M |
| | S4 ✅ | Economy: harvesting and refining, pool, buffers and convoys, power, slots and queues, command cap, repair, start kit, claiming (doc 14; D-135; plan in `plans/S4-economy.md`); the core-kit price (D-189) and the diminishing yield (D-217) | L |
| | S5 ✅ | Combat (doc 15 §4–5; plan in `plans/S5-combat.md`). S5a ✅: weapons, lock, projectiles, multipliers, targeting, engagement. S5b ✅: dome sectors and regeneration, turrets, abilities, module damage, after S4 (D-139) | L |
| | S6 ✅ | Offline and siege rules (docs 12, 15; plan in `plans/S6-offline-and-sieges.md`). S6a ✅: commander records, activity and windows, presence, grace and order lapse, alongside S5b. S6b ✅: breach, Reinforced, siege, Held and Fallen, capture, emergency jump, outpost combat, protections, after S5b (D-172; claiming moved to S4, D-135) | L |
| | S7 | Sensors: visibility sets, signatures, lane traffic, last-known memory, `CommanderView` (doc 16; plan in `plans/S7-sensors.md`) | M |
| | S8 ✅ | Season standings, win, season end (D-020, D-041; plan in `plans/S8-season-end.md`, D-191 to D-198) | S |
| **AI** | A1 | Deputy (doc 13 §4; plan in `plans/A1-deputy.md`, D-199 to D-211; A1a and A1b, D-205) | M |
| | A2 | Strategist economy (plan in `plans/A2-strategist-economy.md`, D-212 to D-218) | L |
| | A3 | Raider director (plan in `plans/A3-raider-director.md`, D-219 to D-225; also the α acceptance runs) | M |
| | A4 | Strategist military | XL |
| | A5 | Personalities, difficulty, schedules | M |
| | A6 | Acceptance and pacing tuning (D-083, D-058, D-070) | L, iterative |
| **Server and network** | N1 ✅ | `nomad-types`, `nomad-protocol` (doc 20 §3, §5) | M |
| | N2 ✅ | Server shell: pacing, SQLite journal and snapshots, recovery, catch-up, journal checkpoints, season end (doc 20 §4, §6, §7; D-109, D-110, D-116; plan in `plans/N2-server-shell.md`) | M |
| | N3 ✅ | QUIC transport, sessions, token, commands and results (plan in `plans/N3-transport-and-sessions.md`; D-120 to D-125, D-150, D-151) | M |
| | N4 | Replication with interest management (doc 16 §6; plan in `plans/N4-replication.md`) | L |
| | N5 | Process model. *Changed by D-236; Proposed in doc 21 §4 (Q237):* the server in Docker Desktop, with a compose file, restart policy, named volume, backups, the start screen's server side, shut down and resume. Replaces "the client starts the server, start with Windows" (D-085) | M |
| | N6 | *Proposed (doc 21 §5–§6; Q233–Q236):* web endpoint: HTTP serving of the client, WebSocket transport for N3's sessions, Host and Origin checks; retires N3's `quinn` endpoint (Q234) | M |
| **Client** | M1 ✅ | Window, camera, grid (phase 1) | — |
| | M2 ✅ | Station voxels (phase 1) | — |
| | M3 ✅ | Emissives (bloom), elevation cues, hover tag (phase 1) | — |
| | M4 ✅ | Shield: globe (D-100), sector ring and rim, scripted hit ripples (phase 1) | — |
| | M5–M6 | Ships and effects, HUD (phase 1). *Proposed (Q232):* M6's HUD panels built in HTML on W2 | L in total |
| | W1 | *Proposed (doc 21 §7–§8; Q230, Q238–Q242):* the web build of the client: wasm target, async startup, assets over HTTP, the browser link for C7, keys and fullscreen, notifications, the image's client stage, wasm in CI. Can start now on the phase-1 fixtures | M |
| | W2 | *Proposed (doc 21 §7.2; Q231, Q232):* the HTML UI layer: Leptos, the view-model binding, the M6 HUD. U1–U7 build on it | M |
| | C7 | Network client and view-model store, replacing fixtures with live data (doc 20 §8) | M |
| | C8 | Selection and orders in TACTICAL; live ships and effects | M |
| **UI** | U1 | SYSTEM and SECTOR views, minimal GALAXY (doc 17 §3–4) | L |
| | U2 | Orders and route preview in strategic views (doc 17 §5) | M |
| | U3 | Build menu, queue, blueprints, economy panel | M |
| | U4 | Doctrine editor, Deputy status | M |
| | U5 | Away report, notifications, window and siege panel | M |
| | U6 | Fleet panel, season panel, start screen and catch-up | M |
| | U7 | Hotkey scheme (doc 17 §7) | S |

### Delivered

| Pull request | Merged | Contents |
|---|---|---|
| Zwaliebaba/Nomad#1 | 2026-11-06 | The complete MVP design (D-001 to D-107); repository standards and CI; S1 simulation core; S2 Universe structure; N1 `nomad-types` and `nomad-protocol`; phase-1 M1 and M2; the WebGL preview tool |
| Zwaliebaba/Nomad#2 | 2026-11-06 | S3 movement, fleets and orders (the rules file, D-104; `nomad-harness --fleets`); the N2 server shell (`create`, `run` and `status`; SQLite journal and snapshots, recovery, catch-up, with the simulation's journaled catch-up mode); phase-1 M3 (bloom, elevation cues, hover tag) and the borderless fullscreen default (D-108); the station spread (D-111, `tools/station-spread`); the round-13 decisions D-112 to D-119 |

## 3. Milestones

*Agreed (D-094); MP-1 added by D-237.*

| Milestone | Contents | Acceptance |
|---|---|---|
| **MVP-α — Headless game** | S2–S8, A1, A2, A3, harness | AI commanders with economy, plus raiders and Deputies, play seasons headlessly. Determinism holds (D-049). The economy pacing targets can be measured, and are within ±50% at this stage. Raids happen. Sieges run end to end when driven by scripted commands. |
| **MVP-β — Playable slice** | N1–N5, C7, C8, U1–U3, U7, plus phase-1 M3–M6 | A human plays in the stage-1 sector against raiders and one economy AI, from the browser against the server in Docker Desktop (D-235, D-236; contents add N6, W1 and W2 once Q230–Q243 are answered). The client–server integration test passes on both operating systems (run locally, D-190). A 3-day playtest with server restarts, PC shutdowns and PC sleep (doc 21 §4.4) loses no state beyond the 1 s journal window, and catch-up works. |
| **MVP-γ — Stage 1 (MVP acceptance, D-038)** | A4, A5, A6, U4–U6; siege, window and away-report UI | **1 human, 3 AI and raiders, for a full season.** See §5. |
| **MP-1 — Multiplayer** (D-237) | Up to 4 humans in the stage-1 sector on a hosted server, AI filling empty seats; hosting, identity, derelict holdings (D-023) and notifications outside the client; contents from its own plan (doc 21 §10) | A full season with up to 4 humans, plus raiders and AI seat fillers. Acceptance is set by MP-1's plan. Placed before δ if Q243 is accepted. |
| **MVP-δ — Stage 2** | Up to 9 AI commanders, two sectors (doc 10 §3) | The AI acceptance criteria hold with 10 commanders. Tick and AI budgets hold at stage-2 scale. A stage-2 playtest. |

## 4. Order of work

*Agreed (D-095; the rules-and-AI order amended by D-139, D-172 and D-205).* Three streams run in parallel:

1. **Rules and AI (the critical path):** S3 → S4 and S5a in parallel (D-139) → S5b, S6a and S8 in parallel (D-172, D-205). Towards α, D-205 sets the rest by dependency:
   - **S6b** after S5b (D-172); **S7** after S6a, in parallel with S6b;
   - **A1a** (the `nomad-ai` crate, outbox, competence gate, inbox, doctrine, direct control, rebuilding, convoy rerouting) after S6a;
   - **A1b** (threat, alert, engagement, abilities, convoy evacuation) after S7 and S5b;
   - **A2** after A1a and S7; **A3** after S6b, S7 and A1a, in parallel with A2.

   After α: A4 → A5 → A6. The AI is the largest risk (RK-03), so it starts as soon as its rules exist. *Status (§2):* S3, S4, S5a, S5b, S6a and S8 are built; S6b, S7 and A1a are next.
2. **Server and network:** N1 and N2 **can start now**, since they need no game rules. Then N3, N4 and N5 follow, once S3 provides real entities to send. *Status:* N1–N3 are built; N4 builds on S7's `CommanderView` (D-182).
3. **Client and UI:** phase-1 M3–M6 continue now, using fixtures. C7 and C8 follow N3 and N4; U1–U7 follow C7. *Status:* M1–M4 are built; M5 and M6 are next.

The streams meet at β (a playable slice) and γ (acceptance). Each package ends merged on the working branch with green CI, as M1 and the simulation core did.

## 5. MVP acceptance (γ)

*Agreed (D-097).*

1. **A season runs end to end:**
   - A full 8-week season runs fast-forwarded in the harness (allowed by D-036 and D-047) and produces a winner by system control (D-020).
   - A human plays a **14-day** stage-1 game in real time without blocking defects.
2. **The AI** meets D-083 in harness runs with 4 commanders.
3. **Pacing:** the time-to-kill targets (D-058) and economy targets (D-070) are within **±25%** in harness measurements.
4. **The offline experience** works: the away report is accurate; no holding core is lost outside its window (D-015); catch-up after PC shutdowns is correct.
5. **Engineering:** CI is green on both operating systems. The determinism tests pass in CI, and the client–server integration and other ignored tests pass locally (D-190). Tick and AI budgets hold at stage-1 scale (doc 11 §4.3, D-083). The README covers installing, running and controls. The design documents match what was built.

## 6. Out of scope for the MVP

*Agreed (D-096). Most items are derived from earlier decisions.*

| Out of scope | Source |
|---|---|
| Hosting: accounts, PostgreSQL, real certificates, Linux deployment, other human players. *They return at MP-1 (D-237).* | D-026, D-029, doc 20, D-236 |
| Alliances and diplomacy, chat | D-026, D-040 |
| Vacation and derelict states | D-038 |
| Late joiners and frontier sectors | Single human |
| Munitions, salvage, finite deposits, exploration, free module placement | D-065, D-071, D-066, D-044, D-067 |
| Galaxy beyond two sectors; full GALAXY view | D-038, doc 17 §8 |
| Notifications outside the client (email or push); inside the browser they are Q241 | Doc 17 §6, doc 21 §7.7 |
| Replays, localisation (English only) | — |
| **Audio** | Proposed: no audio in the MVP. Alerts reach the player through the in-client banner and Windows toasts (doc 17 §6). |

## 7. Risks to the plan

| Risk | Mitigation |
|---|---|
| The AI is weak or slow to build (RK-03) | Staged A1–A6, each measured in the harness; γ needs only 3 AI commanders |
| Pacing problems surface late (RK-02) | MVP-α measures pacing before any UI exists |
| UI volume (doc 17 §9) | U-packages after C7; plain-list panels are acceptable at β |
| Visuals have only been verified on software rendering | The owner runs the client on a real GPU at each client milestone; GPU tests stay runnable locally |
| Scope creep from new ideas | New features go to the out-of-scope list or into a post-MVP design document, not into γ |

## 8. Effect on other documents

- **Design index:** the build status is tracked against §2.
- **`AGENTS.md`:** agents pick work packages from §2 in the order of §4.
